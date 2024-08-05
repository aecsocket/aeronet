use aeronet::{client::DisconnectReason, error::pretty_error};
use aeronet_proto::session::{Session, SessionConfig};
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt, SinkExt,
};
use tracing::{debug, debug_span, Instrument};
use web_time::Instant;
use wtransport::endpoint::{IncomingSession, SessionRequest};

use crate::{
    internal::{self, ConnectionMeta, MIN_MTU},
    runtime::WebTransportRuntime,
};

use super::{
    ClientKey, ConnectionResponse, ServerConfig, ServerError, ToConnected, ToConnecting, ToOpen,
};

pub async fn start(
    runtime: WebTransportRuntime,
    net_config: ServerConfig,
    session_config: SessionConfig,
    send_open: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    let endpoint = wtransport::Endpoint::server(net_config).map_err(ServerError::CreateEndpoint)?;
    let local_addr = endpoint.local_addr().map_err(ServerError::GetLocalAddr)?;

    let (send_closed, mut recv_closed) = oneshot::channel::<()>();
    let (send_connecting, recv_connecting) = mpsc::channel::<ToConnecting>(4);
    send_open
        .send(ToOpen {
            local_addr,
            recv_connecting,
            send_closed,
        })
        .map_err(|_| ServerError::FrontendClosed)?;

    loop {
        let session = futures::select! {
            _ = recv_closed => return Err(ServerError::FrontendClosed),
            x = endpoint.accept().fuse() => x,
        };
        let runtime_clone = runtime.clone();
        let send_connecting = send_connecting.clone();
        let session_config = session_config.clone();
        runtime.spawn(async move {
            if let Err(err) =
                start_handle_session(runtime_clone, session_config, send_connecting, session).await
            {
                debug!("Failed to start handling session: {:#}", pretty_error(&err));
            }
        });
    }
}

async fn start_handle_session(
    runtime: WebTransportRuntime,
    session_config: SessionConfig,
    mut send_connecting: mpsc::Sender<ToConnecting>,
    session: IncomingSession,
) -> Result<(), ServerError> {
    let req = session.await.map_err(ServerError::AwaitSessionRequest)?;

    let (send_key, recv_key) = oneshot::channel::<ClientKey>();
    let (send_conn_resp, recv_conn_resp) = oneshot::channel::<ConnectionResponse>();
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ServerError>>();
    let (send_connected, recv_connected) = oneshot::channel::<ToConnected>();
    send_connecting
        .send(ToConnecting {
            authority: req.authority().to_string(),
            path: req.path().to_string(),
            origin: req.origin().map(ToOwned::to_owned),
            user_agent: req.user_agent().map(ToOwned::to_owned),
            headers: req.headers().clone(),
            send_key,
            send_conn_resp,
            recv_dc,
            recv_connected,
        })
        .await
        .map_err(|_| ServerError::FrontendClosed)?;
    let client_key = recv_key.await.map_err(|_| ServerError::FrontendClosed)?;

    let err = async move {
        let Err(err) =
            handle_session(runtime, session_config, req, recv_conn_resp, send_connected).await
        else {
            unreachable!()
        };
        match &err {
            DisconnectReason::Error(ServerError::FrontendClosed) => {
                debug!("Session closed");
            }
            err => {
                debug!("Session closed: {:#}", pretty_error(err));
            }
        }
        err
    }
    .instrument(debug_span!(
        "session",
        client = tracing::field::display(client_key)
    ))
    .await;
    let _ = send_dc.send(err);
    Ok(())
}

async fn handle_session(
    runtime: WebTransportRuntime,
    session_config: SessionConfig,
    req: SessionRequest,
    recv_conn_resp: oneshot::Receiver<ConnectionResponse>,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ServerError>> {
    debug!("New session request from {}{}", req.authority(), req.path());

    let conn_resp = recv_conn_resp
        .await
        .map_err(|_| ServerError::FrontendClosed)?;
    debug!("Frontend responded to this request with {conn_resp:?}");

    let conn = match conn_resp {
        ConnectionResponse::Accepted => req.accept(),
        ConnectionResponse::Forbidden => {
            req.forbidden().await;
            return Err(ServerError::Rejected.into());
        }
        ConnectionResponse::NotFound => {
            req.not_found().await;
            return Err(ServerError::Rejected.into());
        }
    }
    .await
    .map_err(ServerError::AcceptSessionRequest)?;

    let conn = xwt_wtransport::Connection(conn);
    let Some(mtu) = internal::get_mtu(&conn) else {
        return Err(ServerError::DatagramsNotSupported.into());
    };
    let conn = conn.0;
    let session = Session::new(Instant::now(), session_config, MIN_MTU, mtu)
        .map_err(ServerError::MtuTooSmall)?;

    debug!("Connection opened, forwarding to frontend");

    let (send_meta, recv_meta) = mpsc::channel::<ConnectionMeta>(1);
    let (send_c2s, recv_c2s) = mpsc::channel::<Bytes>(internal::MSG_BUF_CAP);
    let (send_s2c, recv_s2c) = mpsc::unbounded::<Bytes>();
    let (send_local_dc, recv_local_dc) = oneshot::channel::<String>();
    send_connected
        .send(ToConnected {
            remote_addr: conn.remote_address(),
            initial_rtt: conn.rtt(),
            recv_meta,
            recv_c2s,
            send_s2c,
            send_local_dc,
            session,
        })
        .map_err(|_| ServerError::FrontendClosed)?;
    let conn = xwt_wtransport::Connection(conn);

    debug!("Starting connection loop");
    internal::handle_connection(runtime, conn, recv_s2c, send_c2s, send_meta, recv_local_dc)
        .await
        .map_err(|err| err.map_err(From::from))
}
