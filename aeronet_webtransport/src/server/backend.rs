use std::sync::Arc;

use aeronet::{LaneConfig, ProtocolVersion};
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt,
};
use tracing::{debug, debug_span, Instrument};
use wtransport::{endpoint::IncomingSession, Endpoint};

use crate::{
    server::{ClientRequestingKey, ConnectionResponse},
    shared, BackendError, RemoteRequestingInfo, WebTransportServerConfig,
};

use super::{ClientRequesting, OpenInner};

pub async fn open(
    config: WebTransportServerConfig,
    send_open: oneshot::Sender<Result<OpenInner, BackendError>>,
) {
    debug!("Opening backend");
    let endpoint = match Endpoint::server(config.native).map_err(BackendError::CreateEndpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };
    let local_addr = match endpoint.local_addr().map_err(BackendError::GetLocalAddr) {
        Ok(local_addr) => local_addr,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };

    let (mut send_client, recv_client) = mpsc::channel(1);
    let (send_closed, mut recv_closed) = oneshot::channel();
    let _ = send_open.send(Ok(OpenInner {
        local_addr,
        recv_client,
        send_closed,
    }));

    let lanes = Arc::<[LaneConfig]>::from(config.lanes);
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

        let lanes = lanes.clone();
        tokio::spawn(
            handle_incoming(
                session,
                config.version,
                config.max_packet_len,
                lanes,
                send_req,
            )
            .instrument(debug_span!("Session", key = tracing::field::display(key))),
        );
    }
}

async fn handle_incoming(
    session: IncomingSession,
    version: ProtocolVersion,
    max_packet_len: usize,
    lanes: Arc<[LaneConfig]>,
    send_req: oneshot::Sender<Result<ClientRequesting, BackendError>>,
) {
    let req = match session.await.map_err(BackendError::AcceptSessionRequest) {
        Ok(req) => req,
        Err(err) => {
            let _ = send_req.send(Err(err));
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
        info: RemoteRequestingInfo {
            authority: req.authority().to_string(),
            path: req.path().to_string(),
            origin: req.origin().map(ToString::to_string),
            user_agent: req.user_agent().map(ToString::to_string),
            headers: req.headers().clone(),
        },
        send_resp,
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
        ConnectionResponse::Rejected => {
            req.forbidden().await;
            return;
        }
    };

    let mut conn = xwt::current::Connection(conn);
    let (chan_frontend, chan_backend) = match shared::connection_channel::<true>(
        &mut conn,
        version,
        max_packet_len,
        &lanes,
    )
    .await
    {
        Ok(t) => t,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };
    let _ = send_conn.send(Ok(chan_frontend));
    chan_backend.handle(conn).await;
}
