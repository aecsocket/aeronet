//! Server-side transport implementation.

mod backend;
mod frontend;

use {
    crate::{
        internal::{self, ConnectionMeta, InternalSession, SessionError, SessionSendError},
        shared::RawRtt,
    },
    aeronet::{
        client::DisconnectReason,
        stats::{ConnectedAt, MessageStats, RemoteAddr, Rtt},
    },
    aeronet_proto::session::{FatalSendError, MtuTooSmall, OutOfMemory, SendError, Session},
    bytes::Bytes,
    futures::channel::{mpsc, oneshot},
    slotmap::SlotMap,
    std::{collections::HashMap, io, net::SocketAddr},
    web_time::{Duration, Instant},
    wtransport::error::ConnectionError,
};

/// Server network configuration.
pub type ServerConfig = wtransport::ServerConfig;

/// WebTransport implementation of [`ServerTransport`].
///
/// See the [crate-level documentation](crate).
///
/// [`ServerTransport`]: aeronet::server::ServerTransport
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer {
    state: State,
}

#[derive(Debug)]
enum State {
    Closed,
    Opening(Opening),
    Open(Open),
    Closing { reason: String },
}

/// How a [`WebTransportServer`] should respond to a client attempting to
/// connect to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    /// Allow the client to connect.
    Accepted,
    /// 403 Forbidden.
    Forbidden,
    /// 404 Not Found.
    NotFound,
}

/// Error type for [`WebTransportServer::open`], emitted if the server is
/// already opening or open.
#[derive(Debug, Clone, thiserror::Error)]
#[error("not closed")]
pub struct ServerNotClosed;

/// Error type for operations on a [`WebTransportServer`].
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    // frontend
    /// Backend server task was cancelled, dropping the underlying connections.
    #[error("backend closed")]
    BackendClosed,
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
    /// Frontend did not allow this client to complete the connection.
    #[error("rejected by server")]
    Rejected,

    // connection
    /// Lost connection.
    #[error("connection lost")]
    ConnectionLost(#[source] <internal::Connection as xwt_core::session::datagram::Receive>::Error),
}

impl From<SessionError> for ServerError {
    fn from(value: SessionError) -> Self {
        match value {
            SessionError::BackendClosed => Self::BackendClosed,
            SessionError::MtuTooSmall(err) => Self::MtuTooSmall(err),
            SessionError::OutOfMemory(err) => Self::OutOfMemory(err),
            SessionError::FrontendClosed => Self::FrontendClosed,
            SessionError::DatagramsNotSupported => Self::DatagramsNotSupported,
            SessionError::ConnectionLost(err) => Self::ConnectionLost(err),
        }
    }
}

/// Error type for [`WebTransportServer::send`].
///
/// [`WebTransportServer::send`]: aeronet::server::ServerTransport::send
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerSendError {
    /// Attempted to send over a server which is not open.
    #[error("not open")]
    NotOpen,
    /// Attempted to send to a client which is not connected.
    #[error("client not connected")]
    ClientNotConnected,
    /// Failed to buffer up a message for sending, but the connection can still
    /// remain alive.
    #[error(transparent)]
    Trivial(SendError),
    /// Failed to buffer up a message for sending, which also caused the
    /// connection to be closed.
    #[error(transparent)]
    Fatal(FatalSendError),
}

impl From<SessionSendError> for ServerSendError {
    fn from(value: SessionSendError) -> Self {
        match value {
            SessionSendError::Trivial(err) => Self::Trivial(err),
            SessionSendError::Fatal(err) => Self::Fatal(err),
        }
    }
}

slotmap::new_key_type! {
    /// Key uniquely identifying a client in a [`WebTransportServer`].
    ///
    /// If the same physical client disconnects and reconnects (i.e. the same
    /// process), this counts as a new client.
    pub struct ClientKey;
}

/// State of a [`WebTransportServer`] when it is [`ServerState::Opening`].
///
/// [`ServerState::Opening`]: aeronet::server::ServerState::Opening
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
///
/// [`ServerState::Open`]: aeronet::server::ServerState::Open
#[derive(Debug)]
pub struct Open {
    /// Address of the local socket that this server's endpoint is bound to.
    pub local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
    clients: SlotMap<ClientKey, Client>,
    _send_closed: oneshot::Sender<()>,
}

#[derive(Debug)]
enum Client {
    Disconnected,
    Disconnecting { reason: String },
    Connecting(Connecting),
    Connected(Connected),
}

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: HashMap<String, String>,
    recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
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
/// [`ClientState::Connecting`]: aeronet::client::ClientState::Connecting
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
    recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
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
///
/// [`ClientState::Connected`]: aeronet::client::ClientState::Connected
#[derive(Debug)]
pub struct Connected {
    inner: InternalSession,
    recv_dc: oneshot::Receiver<DisconnectReason<ServerError>>,
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
