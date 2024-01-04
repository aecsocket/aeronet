use std::fmt::Debug;

use aeronet::{ByteStats, MessageStats, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use steamworks::{networking_types::NetConnectionEnd, SteamError};

#[derive(Debug, Clone, Default)]
pub struct ConnectionInfo {
    pub msgs_sent: usize,
    pub msgs_recv: usize,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
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
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

/// Error that occurs while processing a Steam networking transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "S::Error: Debug, R::Error: Debug"),
    // TODO: `steamworks::InvalidHandle` should derive Clone
    // Clone(bound = "<P::Send as TryAsBytes>::Error: Clone, <P::Recv as TryFromBytes>::Error: Clone")
)]
pub enum SteamTransportError<S, R>
where
    S: TryAsBytes,
    R: TryFromBytes,
{
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
    #[error("client not connected")]
    NotConnected,
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
    /// Attempted to perform an action on a client which not connected.
    #[error("no client with the given key")]
    NoClient,
    #[error("client not connecting")]
    NotConnecting,
    #[error("client session already accepted/rejected")]
    SessionAlreadyDecided,
    #[error("failed to accept/reject client")]
    DecideSession(#[source] SteamError),

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
