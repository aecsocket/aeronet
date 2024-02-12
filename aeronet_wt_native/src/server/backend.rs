use aeronet::ProtocolVersion;
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt,
};
use tracing::{debug, debug_span, Instrument};
use wtransport::{endpoint::IncomingSession, Endpoint};

use crate::{
    server::{ClientRequestingKey, ConnectionResponse, OpenServerInner},
    shared, BackendError, ClientRequestingInfo, WebTransportServerConfig,
};

use super::ClientRequesting;

pub(super) async fn open(
    config: WebTransportServerConfig,
    send_open: oneshot::Sender<Result<OpenServerInner, BackendError>>,
) {
    debug!("Opening backend");
    let endpoint = match Endpoint::server(config.wt_config).map_err(BackendError::CreateEndpoint) {
        Ok(t) => t,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };
    let local_addr = match endpoint.local_addr().map_err(BackendError::GetLocalAddr) {
        Ok(t) => t,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };

    let (mut send_client, recv_client) = mpsc::channel(1);
    let (_send_closed, mut recv_closed) = oneshot::channel();
    let _ = send_open.send(Ok(OpenServerInner {
        local_addr,
        recv_client,
        _send_closed,
    }));

    loop {
        debug!("Waiting for connections");
        let session = futures::select! {
            session = endpoint.accept().fuse() => session,
            _ = recv_closed => {
                debug!("Closed backend");
                return;
            }
        };

        let (send_key, recv_key) = oneshot::channel();
        let (send_req, recv_req) = oneshot::channel();
        let _ = send_client
            .send(ClientRequestingKey { send_key, recv_req })
            .await;
        let Ok(key) = recv_key.await else { continue };
        debug!("Incoming session {key}");

        tokio::spawn(
            handle_incoming(session, config.version, send_req)
                .instrument(debug_span!("Session", key = tracing::field::display(key))),
        );
    }
}

async fn handle_incoming(
    session: IncomingSession,
    version: ProtocolVersion,
    send_req: oneshot::Sender<Result<ClientRequesting, BackendError>>,
) {
    let req = match session.await {
        Ok(req) => req,
        Err(err) => {
            let _ = send_req.send(Err(BackendError::AcceptSessionRequest(err)));
            return;
        }
    };

    let (send_resp, recv_resp) = oneshot::channel();
    let (send_conn, recv_conn) = oneshot::channel();
    debug!(
        "Connection request from {}{} ({:?})",
        req.authority(),
        req.path(),
        req.origin()
    );
    let _ = send_req.send(Ok(ClientRequesting {
        info: ClientRequestingInfo {
            authority: req.authority().to_string(),
            path: req.path().to_string(),
            origin: req.origin().map(ToString::to_string),
            user_agent: req.user_agent().map(ToString::to_string),
            headers: req.headers().clone(),
        },
        send_resp: Some(send_resp),
        recv_conn,
    }));

    let Ok(resp) = recv_resp.await else { return };
    let conn = match resp {
        ConnectionResponse::Accepted => match req.accept().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = send_conn.send(Err(BackendError::AcceptSession(err)));
                return;
            }
        },
        ConnectionResponse::Forbidden => {
            req.forbidden().await;
            return;
        }
    };

    let (chan_frontend, chan_backend) =
        match shared::connection_channel::<true>(&conn, version).await {
            Ok(t) => t,
            Err(err) => {
                let _ = send_conn.send(Err(err));
                return;
            }
        };
    let _ = send_conn.send(Ok(chan_frontend));
    shared::handle_connection(conn, chan_backend).await
}
