use std::{collections::HashMap, net::SocketAddr};

use aeronet::protocol::ProtocolVersion;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt, SinkExt,
};
use tracing::{debug, debug_span, Instrument};
use wtransport::endpoint::IncomingSession;

use crate::{
    internal,
    server::ConnectionResponse,
    shared::{self, ConnectionStats},
    ty,
};

use super::{ClientKey, ServerBackendError};

#[derive(Debug)]
pub struct Open {
    pub local_addr: SocketAddr,
    pub recv_connecting: mpsc::Receiver<Connecting>,
}

#[derive(Debug)]
pub struct Connecting {
    pub send_key: oneshot::Sender<ClientKey>,
    pub recv_requesting: oneshot::Receiver<Requesting>,
}

#[derive(Debug)]
pub struct Requesting {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
    pub send_conn_resp: oneshot::Sender<ConnectionResponse>,
    pub recv_connected: oneshot::Receiver<Connected>,
}

#[derive(Debug)]
pub struct Connected {
    pub remote_addr: SocketAddr,
    pub initial_stats: ConnectionStats,
    pub recv_c2s: mpsc::Receiver<Bytes>,
    pub send_s2c: mpsc::UnboundedSender<Bytes>,
    pub recv_stats: mpsc::Receiver<ConnectionStats>,
}

pub async fn start(
    native_config: wtransport::ServerConfig,
    version: ProtocolVersion,
    send_open: oneshot::Sender<Open>,
) -> Result<Never, ServerBackendError> {
    debug!("Opening server");
    let endpoint = wtransport::Endpoint::server(native_config)
        .map_err(shared::BackendError::CreateEndpoint)?;

    debug!("Opened server, starting connection loop");
    let local_addr = endpoint
        .local_addr()
        .map_err(shared::BackendError::GetLocalAddr)?;
    let (send_connecting, recv_connecting) = mpsc::channel::<Connecting>(internal::BUFFER_SIZE);
    send_open
        .send(Open {
            local_addr,
            recv_connecting,
        })
        .map_err(|_| shared::BackendError::FrontendClosed)?;

    loop {
        let session = endpoint.accept().await;
        tokio::spawn(start_handle_session(
            version,
            session,
            send_connecting.clone(),
        ));
    }
}

async fn start_handle_session(
    version: ProtocolVersion,
    session: IncomingSession,
    mut send_connecting: mpsc::Sender<Connecting>,
) -> Result<Never, ServerBackendError> {
    let (send_key, recv_key) = oneshot::channel::<ClientKey>();
    let (send_requesting, recv_requesting) = oneshot::channel::<Requesting>();
    send_connecting
        .send(Connecting {
            send_key,
            recv_requesting,
        })
        .await
        .map_err(|_| shared::BackendError::FrontendClosed)?;
    let client_key = recv_key
        .await
        .map_err(|_| shared::BackendError::FrontendClosed)?;

    handle_session(version, session, send_requesting)
        .instrument(debug_span!(
            "Session",
            client = tracing::field::display(client_key)
        ))
        .await
}

async fn handle_session(
    version: ProtocolVersion,
    session: IncomingSession,
    send_requesting: oneshot::Sender<Requesting>,
) -> Result<Never, ServerBackendError> {
    debug!("Receiving new session request");
    let req = session
        .await
        .map_err(ServerBackendError::AwaitSessionRequest)?;

    debug!("New session request from {}{}", req.authority(), req.path());
    let (send_conn_resp, recv_conn_resp) = oneshot::channel::<ConnectionResponse>();
    let (send_connected, recv_connected) = oneshot::channel::<Connected>();
    send_requesting
        .send(Requesting {
            authority: req.authority().to_string(),
            path: req.path().to_string(),
            origin: req.origin().map(ToString::to_string),
            user_agent: req.user_agent().map(ToString::to_string),
            headers: req.headers().clone(),
            send_conn_resp,
            recv_connected,
        })
        .map_err(|_| shared::BackendError::FrontendClosed)?;
    let conn_resp = recv_conn_resp
        .await
        .map_err(|_| shared::BackendError::FrontendClosed)?;

    debug!("Responding to session request with {conn_resp:?}");
    let conn = match conn_resp {
        ConnectionResponse::Accept => req.accept(),
        ConnectionResponse::Forbidden => {
            req.forbidden().await;
            return Err(ServerBackendError::ForceDisconnect);
        }
        ConnectionResponse::NotFound => {
            req.not_found().await;
            return Err(ServerBackendError::ForceDisconnect);
        }
    }
    .await
    .map_err(ServerBackendError::AcceptSessionRequest)?;

    debug!("Connection opened, waiting for managed stream");
    let (mut send_managed, mut recv_managed) = conn
        .accept_bi()
        .await
        .map_err(shared::BackendError::AcceptManaged)?;

    debug!("Managed stream open, negotiating protocol");
    internal::negotiate::server(version, &mut send_managed, &mut recv_managed).await?;

    debug!("Negotiated successfully, forwarding to frontend");
    let (send_c2s, recv_c2s) = mpsc::channel::<Bytes>(internal::BUFFER_SIZE);
    let (send_s2c, recv_s2c) = mpsc::unbounded::<Bytes>();
    let (send_stats, recv_stats) = mpsc::channel::<ConnectionStats>(1);
    send_connected
        .send(Connected {
            remote_addr: conn.remote_address(),
            initial_stats: ConnectionStats::from(&conn),
            recv_c2s,
            send_s2c,
            recv_stats,
        })
        .map_err(|_| shared::BackendError::FrontendClosed)?;

    debug!("Starting connection loop");
    let conn = ty::Connection(conn);
    let send = internal::send(&conn, recv_s2c);
    let recv = internal::recv(&conn, send_c2s, send_stats);
    futures::select! {
        r = send.fuse() => r,
        r = recv.fuse() => r,
    }
    .map_err(From::from)
}
