use std::{io, net::SocketAddr, time::Duration};

use aeronet::{
    protocol::{FragmentationError, ReassemblyError},
    ByteStats, ClientKey, MessageStats, RemoteAddr, Rtt, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use wtransport::error::{ConnectingError, ConnectionError, SendDatagramError};

/// Statistics on a WebTransport client/server connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// See [`RemoteAddr`].
    pub remote_addr: SocketAddr,
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
    pub fn new(remote_addr: SocketAddr, rtt: Duration) -> Self {
        Self {
            remote_addr,
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

impl RemoteAddr for ConnectionInfo {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

/// Error that occurs when processing a [`WebTransportClient`] or
/// [`WebTransportServer`].
///
/// [`WebTransportClient`]: crate::WebTransportClient
/// [`WebTransportServer`]: crate::WebTransportServer
#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = ""))]
pub enum WebTransportError<S: TryAsBytes, R: TryFromBytes> {
    /// An error occurred on the backend which handles the connection loop.
    #[error("backend error")]
    Backend(
        #[from]
        #[source]
        BackendError,
    ),

    /// Failed to encode a message into its byte form.
    #[error("failed to encode message")]
    Encode(#[source] S::Error),

    /// Failed to decode a byte sequence into a message.
    #[error("failed to decode message")]
    Decode(#[source] R::Error),

    #[error("client already connected")]
    AlreadyConnected,
    #[error("client already disconnected")]
    AlreadyDisconnected,
    #[error("client not connected")]
    NotConnected,

    #[error("server already open")]
    AlreadyOpen,
    #[error("server already closed")]
    AlreadyClosed,
    #[error("server not open")]
    NotOpen,
    #[error("no client with key {0}")]
    NoClient(ClientKey),
    #[error("already responded to this session request")]
    AlreadyRespondedToRequest,
}

impl<S: TryAsBytes, R: TryFromBytes> WebTransportError<S, R> {
    /// Creates a [`WebTransportError::Backend`] with source
    /// [`BackendError::Closed`].
    pub fn backend_closed() -> Self {
        Self::Backend(BackendError::Closed)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("closed")]
    Closed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect to server")]
    Connect(#[source] ConnectingError),
    #[error("connection does not support datagrams")]
    DatagramsNotSupported,
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    #[error("lost connection")]
    LostConnection(#[source] ConnectionError),
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),

    #[error("failed to fragment message")]
    Fragment(#[source] FragmentationError),
    #[error("failed to reassemble packet")]
    Reassemble(#[source] ReassemblyError),

    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
}
