use std::{io, net::SocketAddr, time::Duration};

use aeronet::{
    client::ClientKey, ByteStats, MessageStats, RemoteAddr, Rtt, TryAsBytes, TryFromBytes,
};
use aeronet_protocol::{
    LaneRecvError, LaneSendError, NegotiationRequestError, NegotiationResponseError,
    WrongProtocolVersion, NEG_REQUEST_LEN, NEG_RESPONSE_LEN,
};
use derivative::Derivative;
use wtransport::error::ConnectionError;

#[cfg(target_family = "wasm")]
pub type Endpoint = xwt::current::Endpoint;
#[cfg(target_family = "wasm")]
pub type Connecting = xwt::current::Connecting;

#[cfg(not(target_family = "wasm"))]
pub type Endpoint = xwt::current::Endpoint<wtransport::endpoint::endpoint_side::Client>;
#[cfg(not(target_family = "wasm"))]
pub type Connecting = xwt_core::utils::dummy::Connecting<wtransport::Connection>;

pub const MTU: usize = 1200;

/// Statistics on a WebTransport client/server connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// See [`RemoteAddr`].
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    /// See [`Rtt`].
    #[cfg(not(target_family = "wasm"))]
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
    #[cfg(not(target_family = "wasm"))]
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

    #[must_use]
    #[cfg(target_family = "wasm")]
    pub fn new() -> Self {
        Self {
            msgs_sent: 0,
            msgs_recv: 0,
            msg_bytes_sent: 0,
            msg_bytes_recv: 0,
            total_bytes_sent: 0,
            total_bytes_recv: 0,
        }
    }
}

impl From<&xwt::current::Connection> for ConnectionInfo {
    fn from(value: &xwt::current::Connection) -> Self {
        #[cfg(target_family = "wasm")]
        {
            Self::new()
        }
        #[cfg(not(target_family = "wasm"))]
        {
            Self::new(value.0.remote_address(), value.0.rtt())
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl RemoteAddr for ConnectionInfo {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

#[cfg(not(target_family = "wasm"))]
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

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("backend closed")]
    Closed,

    #[cfg(not(target_family = "wasm"))]
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),

    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    #[error("failed to start connection")]
    Connect(#[source] <Endpoint as xwt_core::traits::EndpointConnect>::Error),
    #[error("failed to connect")]
    Connecting(#[source] <Connecting as xwt_core::traits::Connecting>::Error),
    #[error("failed to start opening managed stream")]
    OpeningManaged(#[source] <xwt::current::Connection as xwt_core::traits::OpenBiStream>::Error),
    #[error("failed to open managed stream")]
    OpenManaged(
        #[source] <xwt::current::OpeningBiStream as xwt_core::traits::OpeningBiStream>::Error,
    ),
    #[error("failed to accept managed stream")]
    AcceptManaged(#[source] <xwt::current::Connection as xwt_core::traits::AcceptBiStream>::Error),
    #[error("failed to send on managed stream")]
    SendManaged(
        #[source]
        <xwt::current::SendStream as xwt_core::io::WriteChunk<xwt_core::io::chunk::U8>>::Error,
    ),
    #[error("failed to receive on managed stream")]
    RecvManaged(#[source] <xwt::current::RecvStream as xwt_core::io::Read>::Error),
    #[error("managed stream closed")]
    ManagedStreamClosed,

    #[error("lost connection")]
    LostConnection(#[source] <xwt::current::Connection as xwt_core::datagram::Receive>::Error),
    #[error("failed to send datagram")]
    SendDatagram(#[source] <xwt::current::Connection as xwt_core::datagram::Send>::Error),

    #[error("invalid negotiation request length - expected {NEG_REQUEST_LEN}, was {len} bytes")]
    NegotiateRequestLength { len: usize },
    #[error("failed to read negotiation request")]
    ReadNegotiateRequest(#[source] NegotiationRequestError),
    #[error("invalid negotiation response length - expected {NEG_RESPONSE_LEN}, was {len} bytes")]
    NegotiateResponseLength { len: usize },
    #[error("failed to read negotiation response")]
    ReadNegotiateResponse(#[source] NegotiationResponseError),
    #[error("wrong protocol version")]
    WrongProtocolVersion(#[source] WrongProtocolVersion),

    #[cfg(not(target_family = "wasm"))]
    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[cfg(not(target_family = "wasm"))]
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),

    #[error("sending on lane")]
    LaneSend(#[source] LaneSendError),
    #[error("receiving on lane")]
    LaneRecv(#[source] LaneRecvError),
}

/// Error that occurs when interacting with the [`WebTransportClient`] or
/// [`WebTransportServer`] frontends.
///
/// [`WebTransportClient`]: crate::WebTransportClient
/// [`WebTransportServer`]: crate::WebTransportServer
#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = ""))]
pub enum WebTransportError<S: TryAsBytes, R: TryFromBytes> {
    #[error("backend error")]
    Backend(#[from] BackendError),
    #[error("failed to convert message to bytes")]
    AsBytes(#[source] S::Error),
    #[error("failed to convert bytes to message")]
    FromBytes(#[source] R::Error),

    #[error("client not connected")]
    NotConnected,
    #[error("client already connected")]
    AlreadyConnected,
    #[error("client already disconnected")]
    AlreadyDisconnected,

    #[error("server already open")]
    AlreadyOpen,
    #[error("server already closed")]
    AlreadyClosed,
    #[error("server not open")]
    NotOpen,
    #[error("no client with key {client}")]
    NoClient { client: ClientKey },
    #[error("client {client} is already connected")]
    ClientAlreadyConnected { client: ClientKey },
    #[error("already responded to this session request")]
    AlreadyRespondedToRequest,
}

/*
/// Error that occurs when interacting with the [`WebTransportClient`] or
/// [`WebTransportServer`] frontends.
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
    #[error("no client with key {client}")]
    NoClient { client: ClientKey },
    #[error("client {client} is already connected")]
    ClientAlreadyConnected { client: ClientKey },
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
    #[error("backend closed")]
    Closed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect to server")]
    Connect(#[source] xwt_error::Connect<_>),
    #[error("connection does not support datagrams")]
    DatagramsNotSupported,
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),

    #[error("failed to start opening managed stream")]
    OpeningManaged(#[source] ConnectionError),
    #[error("failed to open managed stream")]
    OpenManaged(#[source] StreamOpeningError),
    #[error("failed to accept managed stream")]
    AcceptManaged(#[source] ConnectionError),
    #[error("failed to send along managed stream")]
    SendManaged(#[source] StreamWriteError),
    #[error("failed to receive from managed stream")]
    RecvManaged(#[source] StreamReadError),
    #[error("managed stream closed")]
    ManagedStreamClosed,

    #[error(
        "invalid negotiation request length - expected {}, was {len} bytes",
        Negotiation::REQUEST_LEN
    )]
    NegotiateRequestLength { len: usize },
    #[error("failed to read negotiation request")]
    ReadNegotiateRequest(#[source] NegotiationRequestError),
    #[error(
        "invalid negotiation response length - expected {}, was {len} bytes",
        Negotiation::RESPONSE_LEN
    )]
    NegotiateResponseLength { len: usize },
    #[error("failed to read negotiation response")]
    ReadNegotiateResponse(#[source] NegotiationResponseError),
    #[error("wrong protocol version")]
    WrongVersion(#[source] WrongProtocolVersion),

    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentationError),

    #[error("lost connection")]
    LostConnection(#[source] ConnectionError),
    #[error("failed to read lane")]
    ReadLane,
    #[error("received message on invalid lane index {lane_index}")]
    RecvOnInvalidLane { lane_index: usize },
    #[error("failed to reassemble packet")]
    Reassemble(#[source] ReassemblyError),

    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
}
*/
