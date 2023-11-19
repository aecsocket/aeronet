use std::{io, net::SocketAddr, time::Duration};

use aeronet::{ChannelKey, Message, RemoteAddr, Rtt, TryFromBytes, TryIntoBytes};
use wtransport::{
    error::{
        ConnectionError, SendDatagramError, StreamOpeningError, StreamReadError, StreamWriteError,
    },
    Connection,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelId {
    Datagram,
    Stream(usize),
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
    /// Failed to create the [`wtransport::Endpoint`].
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    /// Failed to receive an incoming session.
    #[error("failed to receive incoming session")]
    IncomingSession(#[source] ConnectionError),
    /// Failed to accept an incoming session.
    #[error("failed to accept incoming session")]
    AcceptSession(#[source] ConnectionError),
    #[error("failed to establish channel {channel}")]
    Establish {
        /// The channel on which the error occurred.
        channel: C,
        /// The error that occurred.
        #[source]
        source: ChannelEstablishError,
    },
    /// An error occurred while sending on an app channel.
    #[error("sending on {channel:?}")]
    Send {
        /// The channel on which the error occurred.
        channel: C,
        /// The error that occurred.
        #[source]
        source: ChannelSendError<S>,
    },
    #[error("receiving on {channel:?}")]
    Recv {
        channel: ChannelId,
        #[source]
        source: ChannelRecvError<R>,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelEstablishError {
    /// Failed to request to open a bidirectional stream.
    #[error("failed to request to open stream")]
    RequestOpenStream(#[source] ConnectionError),
    /// Failed to open a bidirectional stream.
    #[error("failed to open stream")]
    OpenStream(#[source] StreamOpeningError),
}

/// Error that occurrs when sending data on a specific channel.
#[derive(Debug, thiserror::Error)]
pub enum ChannelSendError<S>
where
    S: Message + TryIntoBytes,
{
    /// Failed to send a datagram.
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
    /// Failed to write into a bidirectional stream.
    #[error("failed to write stream")]
    WriteStream(#[source] StreamWriteError),
    /// Failed to serialize data using [`TryIntoBytes::try_into_bytes`].
    #[error("failed to serialize data")]
    Serialize(#[source] S::Error),
}

/// Error that occurrs when sending data on a specific channel.
#[derive(Debug, thiserror::Error)]
pub enum ChannelRecvError<R>
where
    R: Message + TryFromBytes,
{
    #[error("failed to recv datagram")]
    RecvDatagram(#[source] ConnectionError),
    /// Failed to read from a bidirectional stream.
    #[error("failed to read stream")]
    ReadStream(#[source] StreamReadError),
    /// Failed to deserialize data using [`TryFromBytes::try_from_bytes`].
    #[error("failed to deserialize data")]
    Deserialize(#[source] R::Error),
}

impl<S, R, C> WebTransportError<S, R, C>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    /// Creates a [`WebTransportError::Establish`] given the channel and source
    /// error.
    pub fn establish(channel: C, source: ChannelEstablishError) -> Self {
        Self::Establish { channel, source }
    }

    /// Creates a [`WebTransportError::Sending`] given the channel and source
    /// error.
    pub fn send(channel: C, source: ChannelSendError<S>) -> Self {
        Self::Send { channel, source }
    }

    /// Creates a [`WebTransportError::Receiving`] given the channel and source
    /// error.
    pub fn recv(channel: ChannelId, source: ChannelRecvError<R>) -> Self {
        Self::Recv { channel, source }
    }
}
