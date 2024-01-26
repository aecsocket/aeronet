use std::net::SocketAddr;

use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt,
};
use tracing::{debug, debug_span, Instrument};
use wtransport::{
    endpoint::{endpoint_side, IncomingSession},
    Endpoint, ServerConfig,
};

use crate::{
    server::{ClientIncoming, OpenServerInner},
    shared, BackendError, SessionResponse,
};

use super::ClientRequesting;

pub(super) async fn open(
    config: ServerConfig,
    send_open: oneshot::Sender<Result<OpenServerInner, BackendError>>,
) {
    debug!("Opening backend");
    let (endpoint, local_addr) = match create(config) {
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
            .send(ClientIncoming { send_key, recv_req })
            .await;
        let Ok(key) = recv_key.await else { continue };

        tokio::spawn(
            handle_incoming(session, send_req)
                .instrument(debug_span!("Client", key = tracing::field::display(key))),
        );
    }
}

fn create(
    config: ServerConfig,
) -> Result<(Endpoint<endpoint_side::Server>, SocketAddr), BackendError> {
    let endpoint = Endpoint::server(config).map_err(BackendError::CreateEndpoint)?;
    let local_addr = endpoint.local_addr().map_err(BackendError::GetLocalAddr)?;

    Ok((endpoint, local_addr))
}

async fn handle_incoming(
    session: IncomingSession,
    send_req: oneshot::Sender<Result<ClientRequesting, BackendError>>,
) {
    debug!("Incoming session");
    let req = match session.await {
        Ok(req) => req,
        Err(err) => {
            let _ = send_req.send(Err(BackendError::AcceptSessionRequest(err)));
            return;
        }
    };

    let (send_resp, recv_resp) = oneshot::channel();
    let (send_conn, recv_conn) = oneshot::channel();
    let _ = send_req.send(Ok(ClientRequesting {
        authority: req.authority().to_string(),
        path: req.path().to_string(),
        origin: req.origin().map(ToString::to_string),
        user_agent: req.user_agent().map(ToString::to_string),
        send_resp,
        recv_conn,
    }));

    let Ok(resp) = recv_resp.await else { return };
    let conn = match resp {
        SessionResponse::Accept => match req.accept().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = send_conn.send(Err(BackendError::AcceptSession(err)));
                return;
            }
        },
        SessionResponse::Forbidden => {
            req.forbidden().await;
            return;
        }
    };

    shared::start_connection(conn, send_conn).await
}
