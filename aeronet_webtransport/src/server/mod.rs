mod backend;
mod frontend;

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    io,
    net::SocketAddr,
};

use aeronet::{
    client::ClientState,
    server::ServerState,
    stats::{MessageStats, RemoteAddr, Rtt},
};
use aeronet_proto::session::{OutOfMemory, SendError, Session, SessionConfig};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use web_time::Duration;
use wtransport::error::ConnectionError;

use crate::internal;

pub type ServerConfig = wtransport::ServerConfig;

#[derive(Derivative, Default)]
#[derivative(Debug = "transparent")]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer {
    state: State,
}

type State = ServerState<Opening, Open>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    Accept,
    Forbidden,
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    // frontend
    #[error("backend closed")]
    BackendClosed,
    #[error("already opening or open")]
    AlreadyOpen,
    #[error("already closed")]
    AlreadyClosed,
    #[error("not open")]
    NotOpen,
    #[error("client not connected")]
    ClientNotConnected,
    #[error("client not connecting")]
    ClientNotConnecting,
    #[error("already responded to this client's connection request")]
    AlreadyResponded,
    #[error(transparent)]
    Send(SendError),
    #[error(transparent)]
    OutOfMemory(OutOfMemory),

    // backend
    #[error("frontend closed")]
    FrontendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error("server forced disconnect")]
    ForceDisconnect,

    // connection
    #[error("connection lost")]
    ConnectionLost(#[source] <internal::Connection as xwt_core::session::datagram::Receive>::Error),
    #[error("failed to send datagram")]
    SendDatagram(#[source] <internal::Connection as xwt_core::session::datagram::Send>::Error),
}

slotmap::new_key_type! {
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
pub struct Opening {
    recv_open: oneshot::Receiver<ToOpen>,
    recv_err: oneshot::Receiver<ServerError>,
    session_config: SessionConfig,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
    send_closed: oneshot::Sender<()>,
}

#[derive(Debug)]
pub struct Open {
    pub local_addr: SocketAddr,
    session_config: SessionConfig,
    recv_connecting: mpsc::Receiver<ToConnecting>,
    clients: SlotMap<ClientKey, Client>,
    _send_closed: oneshot::Sender<()>,
}

type Client = ClientState<Connecting, Connected>;

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: HashMap<String, String>,
    recv_err: oneshot::Receiver<ServerError>,
    send_key: oneshot::Sender<ClientKey>,
    send_conn_resp: oneshot::Sender<ConnectionResponse>,
    recv_connected: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
pub struct Connecting {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
    recv_err: oneshot::Receiver<ServerError>,
    send_conn_resp: Option<oneshot::Sender<ConnectionResponse>>,
    recv_connected: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    remote_addr: SocketAddr,
    initial_rtt: Duration,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_c2s: mpsc::Receiver<Bytes>,
    send_s2c: mpsc::UnboundedSender<Bytes>,
}

#[derive(Debug)]
pub struct Connected {
    pub remote_addr: SocketAddr,
    pub rtt: Duration,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
    recv_err: oneshot::Receiver<ServerError>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_c2s: mpsc::Receiver<Bytes>,
    send_s2c: mpsc::UnboundedSender<Bytes>,
    session: Session,
}

impl Rtt for Connected {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

impl RemoteAddr for Connected {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}
