use std::{fmt::Debug, time::Duration};

use aeronet::{ByteStats, ClientKey, MessageStats, Rtt, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use steamworks::{networking_types::NetConnectionEnd, SteamError};

/// Statistics on a Steamworks client/server connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// See [`Rtt`].
    pub rtt: Duration,
    /// See [`MessageStats::msgs_sent`].
    pub msgs_sent: usize,
    /// See [`MessageStats::msgs_recv`].
    pub msgs_recv: usize,
    /// See [`ByteStats::msg_bytes_sent`].
    pub msg_bytes_sent: usize,
    /// See [`ByteStats::msg_bytes_recv`].
    pub msg_bytes_recv: usize,
    /// See [`ByteStats::total_bytes_sent`].
    pub total_bytes_sent: usize,
    /// See [`ByteStats::total_bytes_recv`].
    pub total_bytes_recv: usize,
}

impl ConnectionInfo {
    #[must_use]
    pub fn new(rtt: Duration) -> Self {
        Self {
            rtt,
            msgs_sent: 0,
            msgs_recv: 0,
            msg_bytes_sent: 0,
            msg_bytes_recv: 0,
            total_bytes_sent: 0,
            total_bytes_recv: 0,
        }
    }
}

impl Rtt for ConnectionInfo {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl MessageStats for ConnectionInfo {
    fn msgs_sent(&self) -> usize {
        self.msgs_sent
    }

    fn msgs_recv(&self) -> usize {
        self.msgs_recv
    }
}

impl ByteStats for ConnectionInfo {
    fn msg_bytes_recv(&self) -> usize {
        self.msg_bytes_recv
    }

    fn msg_bytes_sent(&self) -> usize {
        self.msg_bytes_sent
    }

    fn total_bytes_sent(&self) -> usize {
        self.total_bytes_sent
    }

    fn total_bytes_recv(&self) -> usize {
        self.total_bytes_recv
    }
}

/// Error that occurs while processing a Steam networking transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "S::Error: Debug, R::Error: Debug"),
    // TODO: `steamworks::InvalidHandle` should derive Clone
    // Clone(bound = "<P::Send as TryAsBytes>::Error: Clone, <P::Recv as TryFromBytes>::Error: Clone")
)]
pub enum SteamTransportError<S: TryAsBytes, R: TryFromBytes> {
    #[error("internal error")]
    InternalError,

    // client
    /// Attempted to disconnect the client while it was already disconnected.
    #[error("client already disconnected")]
    AlreadyDisconnected,
    /// Attempted to establish a new connection while the client was already
    /// connected to a server.
    #[error("client already connected")]
    AlreadyConnected,
    /// Attempted to perform an action which requires a connection, while no
    /// connection is established.
    #[error("client {client} not connected")]
    NotConnected { client: ClientKey },
    /// Failed to start connecting the client to the given remote.
    #[error("client failed to start connecting")]
    StartConnecting,
    #[error("client connection rejected by server")]
    ConnectionRejected,

    // server
    /// Attempted to close the server while it was already closed.
    #[error("already closed")]
    AlreadyClosed,
    /// Attempted to open the server while it was already opening or open.
    #[error("already open")]
    AlreadyOpen,
    /// Attempted to perform an action which requires the server to be open
    /// while it is not.
    #[error("server not open")]
    NotOpen,
    /// Failed to create a listen socket to receive incoming connections on.
    #[error("failed to create listen socket")]
    CreateListenSocket,

    // server-side clients
    #[error("no client with key {client}")]
    NoClient { client: ClientKey },
    #[error("client {client} is already connected")]
    ClientAlreadyConnected { client: ClientKey },
    #[error("already responded to this session request")]
    AlreadyRespondedToRequest,

    // connect
    /// Failed to configure the lanes of the connection.
    #[error("failed to configure lanes")]
    ConfigureLanes(#[source] SteamError),
    #[error("disconnected: {0:?}")]
    Disconnected(NetConnectionEnd),
    #[error("lost connection")]
    ConnectionLost,

    // transport
    #[error("failed to serialize message")]
    Serialize(#[source] S::Error),
    #[error("failed to send message")]
    Send(#[source] SteamError),
    #[error("failed to deserialize message")]
    Deserialize(#[source] R::Error),
}

pub enum BackendError {}
