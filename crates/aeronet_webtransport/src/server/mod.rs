//! Server-side transport implementation.

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
    stats::{ConnectedAt, MessageStats, RemoteAddr, Rtt},
};
use aeronet_proto::session::{FatalSendError, MtuTooSmall, OutOfMemory, SendError, Session};
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use web_time::{Duration, Instant};
use wtransport::error::ConnectionError;

use crate::{
    internal::{self, ConnectionInner, ConnectionMeta, InternalError},
    shared::RawRtt,
};

/// Server network configuration.
pub type ServerConfig = wtransport::ServerConfig;

/// WebTransport implementation of [`ServerTransport`].
///
/// See the [crate-level documentation](crate).
///
/// [`ServerTransport`]: aeronet::server::ServerTransport
#[derive(Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer {
    state: State,
    /// See [`ClientTransport::default_disconnect_reason`].
    pub default_disconnect_reason: Option<String>,
}

type State = ServerState<Opening, Open>;

/// How a [`WebTransportServer`] should respond to a client attempting to
/// connect to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    /// Allow the client to connect.
    Accept,
    /// 403 Forbidden.
    Forbidden,
    /// 404 Not Found.
    NotFound,
}

/// Error type for operations on a [`WebTransportServer`].
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    // frontend
    /// Backend server task was cancelled, dropping the underlying connections.
    #[error("backend closed")]
    BackendClosed,
    /// Server is already opening or open.
    #[error("already opening or open")]
    AlreadyOpen,
    /// Server is already closed.
    #[error("already closed")]
    AlreadyClosed,
    /// Server is not open.
    #[error("not open")]
    NotOpen,
    /// Given client is not connected.
    #[error("client not connected")]
    ClientNotConnected,
    /// Given client is not connecting.
    #[error("client not connecting")]
    ClientNotConnecting,
    /// Already responded to this client's connection request.
    #[error("already responded to this client's connection request")]
    AlreadyResponded,
    /// See [`SendError`].
    #[error(transparent)]
    Send(SendError),
    /// See [`FatalSendError`].
    #[error(transparent)]
    FatalSend(FatalSendError),
    /// See [`OutOfMemory`].
    #[error(transparent)]
    OutOfMemory(OutOfMemory),

    // backend
    /// Server frontend was closed.
    #[error("frontend closed")]
    FrontendClosed,
    /// Failed to create the endpoint for listening to connections from.
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    /// Failed to get our endpoint's local socket address.
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),
    /// Failed to await the client's session request.
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    /// Failed to accept the client's session request.
    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    /// Established a connection with the client, but it does not support
    /// datagrams.
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,
    /// Client supports datagrams, but the maximum datagram size it supports is
    /// too small for us.
    #[error("connection MTU too small")]
    MtuTooSmall(#[source] MtuTooSmall),
    /// Frontend forced this client to disconnect.
    #[error("server forced disconnect")]
    ForceDisconnect,

    // connection
    /// Lost connection.
    #[error("connection lost")]
    ConnectionLost(#[source] <internal::Connection as xwt_core::session::datagram::Receive>::Error),
}

impl From<InternalError<Self>> for ServerError {
    fn from(value: InternalError<Self>) -> Self {
        match value {
            InternalError::Spec(err) => err,
            InternalError::BackendClosed => Self::BackendClosed,
            InternalError::MtuTooSmall(err) => Self::MtuTooSmall(err),
            InternalError::OutOfMemory(err) => Self::OutOfMemory(err),
            InternalError::Send(err) => Self::Send(err),
            InternalError::FatalSend(err) => Self::FatalSend(err),
            InternalError::FrontendClosed => Self::FrontendClosed,
            InternalError::DatagramsNotSupported => Self::DatagramsNotSupported,
            InternalError::ConnectionLost(err) => Self::ConnectionLost(err),
        }
    }
}

slotmap::new_key_type! {
    /// Key uniquely identifying a client in a [`WebTransportServer`].
    ///
    /// If the same physical client disconnects and reconnects (i.e. the same
    /// computer), this counts as a new client.
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// State of a [`WebTransportServer`] when it is [`ServerState::Opening`].
#[derive(Debug)]
pub struct Opening {
    recv_open: oneshot::Receiver<ToOpen>,
    recv_err: oneshot::Receiver<ServerError>,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
    send_closed: oneshot::Sender<()>,
}

/// State of a [`WebTransportServer`] when it is [`ServerState::Open`].
#[derive(Debug)]
pub struct Open {
    /// Address of the local socket that this server's endpoint is bound to.
    pub local_addr: SocketAddr,
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

/// State of a client connected to a [`WebTransportServer`] when it is
/// [`ClientState::Connecting`].
///
/// After receiving a [`ServerEvent::Connecting`], use the information in this
/// to determine whether to accept or to reject this client.
///
/// [`ServerEvent::Connecting`]: aeronet::server::ServerEvent::Connecting
#[derive(Debug)]
pub struct Connecting {
    /// `:authority` field of the request.
    pub authority: String,
    /// `:path` field of the request.
    pub path: String,
    /// `origin` field of the request.
    pub origin: Option<String>,
    /// `user-agent` field of the request.
    pub user_agent: Option<String>,
    /// All headers present in the request.
    pub headers: HashMap<String, String>,
    recv_err: oneshot::Receiver<ServerError>,
    send_conn_resp: Option<oneshot::Sender<ConnectionResponse>>,
    recv_connected: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    remote_addr: SocketAddr,
    initial_rtt: Duration,
    recv_meta: mpsc::Receiver<ConnectionMeta>,
    recv_c2s: mpsc::Receiver<Bytes>,
    send_s2c: mpsc::UnboundedSender<Bytes>,
    send_local_dc: oneshot::Sender<String>,
    session: Session,
}

/// State of a client connected to a [`WebTransportServer`] when it is
/// [`ClientState::Connected`].
#[derive(Debug)]
pub struct Connected {
    inner: ConnectionInner<ServerError>,
}

impl Connected {
    /// Provides access to the underlying [`Session`] for reading more detailed
    /// network statistics.
    #[must_use]
    pub const fn session(&self) -> &Session {
        &self.inner.session
    }
}

impl ConnectedAt for Connected {
    fn connected_at(&self) -> Instant {
        self.session().connected_at()
    }
}

impl Rtt for Connected {
    fn rtt(&self) -> Duration {
        self.session().rtt().get()
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.session().bytes_sent()
    }

    fn bytes_recv(&self) -> usize {
        self.session().bytes_recv()
    }
}

#[cfg(not(target_family = "wasm"))]
impl RemoteAddr for Connected {
    fn remote_addr(&self) -> SocketAddr {
        self.inner.remote_addr
    }
}

#[cfg(not(target_family = "wasm"))]
impl RawRtt for Connected {
    fn raw_rtt(&self) -> Duration {
        self.inner.raw_rtt
    }
}
