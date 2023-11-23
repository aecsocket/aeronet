use std::{io, net::SocketAddr, time::Duration};

use aeronet::{ChannelKey, Message, RemoteAddr, Rtt, TryFromBytes, TryIntoBytes};
use wtransport::{
    error::{
        ConnectionError, SendDatagramError, StreamOpeningError, StreamReadError, StreamWriteError,
    },
    Connection,
};

slotmap::new_key_type! {
    /// Key type used to uniquely identify a client connected to a
    /// [`WebTransportServer`].
    pub struct ClientKey;
}

/// Statistics on the network state of a [`Connection`] managed by an endpoint.
///
/// This serves as a snapshot of network stats, not a live updating value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// The round-trip time of the connection as defined by [`Rtt`].
    ///
    /// See [`Connection::rtt`]
    pub rtt: Duration,
    /// The remote address of this connection as defined by [`RemoteAddr`].
    ///
    /// See [`Connection::remote_address`].
    pub remote_addr: SocketAddr,
    /// See [`Connection::max_datagram_size`].
    pub max_datagram_size: Option<usize>,
}

impl EndpointInfo {
    /// Creates a snapshot of network stats from a given connection.
    pub fn from_connection(conn: &Connection) -> Self {
        Self {
            rtt: conn.rtt(),
            remote_addr: conn.remote_address(),
            max_datagram_size: conn.max_datagram_size(),
        }
    }
}

impl Rtt for EndpointInfo {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl RemoteAddr for EndpointInfo {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

/// Error that occurs when processing a WebTransport transport implementation.
#[derive(Debug, thiserror::Error)]
pub enum WebTransportError<S, R, C>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    /// The backend that handles connections asynchronously was shut down.
    #[error("backend closed")]
    BackendClosed,
    /// Attempted to open the backend while it was already open.
    #[error("backend already open")]
    BackendOpen,
    /// Failed to create the [`wtransport::Endpoint`].
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    /// Failed to receive an incoming session.
    #[error("failed to receive incoming session")]
    IncomingSession(#[source] ConnectionError),
    /// Failed to accept an incoming session.
    #[error("failed to accept incoming session")]
    AcceptSession(#[source] ConnectionError),
    /// An error occurred while processing datagrams not bound to a specific
    /// channel.
    #[error("on datagram channel")]
    OnDatagram(#[source] ChannelError<S, R>),
    /// An error occurred while processing a channel.
    #[error("on {0:?}")]
    OnChannel(C, #[source] ChannelError<S, R>),
    /// Attempted to perform an operation on a client which does not exist.
    #[error("no client with key {0:?}")]
    NoClient(ClientKey),
    /// Attempted to perform an operation on a client which is not connected
    /// yet.
    #[error("client {0:?} has not connected yet")]
    NotConnected(ClientKey),
}

/// Error that occurs while processing a channel, either datagrams or QUIC
/// streams.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError<S, R>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    // establish
    /// Failed to request to open a bidirectional stream.
    #[error("failed to request to open stream")]
    RequestOpenStream(#[source] ConnectionError),
    /// Failed to open a bidirectional stream.
    #[error("failed to open stream")]
    OpenStream(#[source] StreamOpeningError),
    /// Failed to accept an incoming bidirectional stream request.
    #[error("failed to open stream")]
    AcceptStream(#[source] ConnectionError),

    // send
    /// Failed to send a datagram to the other side.
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
    /// Failed to write into a bidirectional stream.
    #[error("failed to write stream")]
    WriteStream(#[source] StreamWriteError),
    /// Failed to serialize data using [`TryIntoBytes::try_into_bytes`].
    #[error("failed to serialize data")]
    Serialize(#[source] S::Error),

    // receive
    /// Failed to receive a datagram from the other side.
    #[error("failed to recv datagram")]
    RecvDatagram(#[source] ConnectionError),
    /// Failed to read from a bidirectional stream.
    #[error("failed to read stream")]
    ReadStream(#[source] StreamReadError),
    /// Failed to deserialize data using [`TryFromBytes::try_from_bytes`].
    #[error("failed to deserialize data")]
    Deserialize(#[source] R::Error),
}
