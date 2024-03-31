use std::time::Duration;

use aeronet::{
    message::{TryFromBytes, TryIntoBytes},
    stats::{ByteStats, MessageStats, Rtt},
};
use aeronet_proto::{message, negotiate};
use derivative::Derivative;

use crate::ClientKey;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type Endpoint = xwt::current::Endpoint;
        pub type Connecting = xwt::current::Connecting;
        pub type OpeningBiStream = xwt_core::utils::dummy::OpeningBiStream<xwt::current::Connection>;
    } else {
        pub type Endpoint = xwt::current::Endpoint<wtransport::endpoint::endpoint_side::Client>;
        pub type Connecting = xwt_core::utils::dummy::Connecting<wtransport::Connection>;
        pub type OpeningBiStream = xwt::current::OpeningBiStream;
    }
}

pub const MTU: usize = 1200;

/// Statistics on a WebTransport client/server connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// See [`RemoteAddr`].
    ///
    /// [`RemoteAddr`]: aeronet::RemoteAddr
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: std::net::SocketAddr,
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

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
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

        impl From<&xwt::current::Connection> for ConnectionInfo {
            fn from(_: &xwt::current::Connection) -> Self {
                Self::new(Duration::ZERO) // TODO
            }
        }
    } else {
        impl ConnectionInfo {
            #[must_use]
            #[cfg(not(target_family = "wasm"))]
            pub fn new(remote_addr: std::net::SocketAddr, rtt: Duration) -> Self {
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

        impl From<&xwt::current::Connection> for ConnectionInfo {
            fn from(value: &xwt::current::Connection) -> Self {
                Self::new(value.0.remote_address(), value.0.rtt())
            }
        }

        impl aeronet::stats::RemoteAddr for ConnectionInfo {
            fn remote_addr(&self) -> std::net::SocketAddr {
                self.remote_addr
            }
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

/// Error that occurs when interacting with the [`WebTransportClient`] or
/// [`WebTransportServer`] frontends.
///
/// [`WebTransportClient`]: crate::WebTransportClient
/// [`WebTransportServer`]: crate::WebTransportServer
#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = ""))]
pub enum WebTransportError<S: TryIntoBytes, R: TryFromBytes> {
    #[error("backend error")]
    Backend(#[from] BackendError),
    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] S::Error),
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
    #[error("no client with key {client_key}")]
    NoClient { client_key: ClientKey },
    #[error("client {client_key} is already connected")]
    ClientAlreadyConnected { client_key: ClientKey },
    #[error("already responded to this session request")]
    AlreadyRespondedToRequest,
}

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        #[derive(Debug, Clone)]
        pub struct JsError(pub String);

        impl std::fmt::Display for JsError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::error::Error for JsError {}

        impl std::ops::Deref for JsError {
            type Target = String;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl From<xwt::current::Error> for JsError {
            fn from(value: xwt::current::Error) -> Self {
                Self(format!("{:?}", value.0))
            }
        }

        impl From<std::convert::Infallible> for JsError {
            fn from(_: std::convert::Infallible) -> Self {
                unreachable!()
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("backend closed")]
    Closed,

    #[cfg(not(target_family = "wasm"))]
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] std::io::Error),
    #[cfg(not(target_family = "wasm"))]
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] std::io::Error),

    #[error("failed to start connection")]
    Connect(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <Endpoint as xwt_core::traits::EndpointConnect>::Error,
    ),
    #[error("failed to connect")]
    Connecting(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <Connecting as xwt_core::traits::Connecting>::Error,
    ),
    #[error("failed to start opening managed stream")]
    OpeningManaged(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <xwt::current::Connection as xwt_core::traits::OpenBiStream>::Error,
    ),
    #[error("failed to open managed stream")]
    OpenManaged(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <OpeningBiStream as xwt_core::traits::OpeningBiStream>::Error,
    ),
    #[error("failed to accept managed stream")]
    AcceptManaged(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <xwt::current::Connection as xwt_core::traits::AcceptBiStream>::Error,
    ),
    #[error("failed to send on managed stream")]
    SendManaged(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <xwt::current::SendStream as xwt_core::io::Write>::Error,
    ),
    #[error("failed to receive on managed stream")]
    RecvManaged(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <xwt::current::RecvStream as xwt_core::io::Read>::Error,
    ),
    #[error("managed stream closed")]
    ManagedStreamClosed,

    #[error("failed to receive datagram")]
    RecvDatagram(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <xwt::current::Connection as xwt_core::datagram::Receive>::Error,
    ),
    #[error("failed to send datagram")]
    SendDatagram(
        #[source]
        #[cfg(target_family = "wasm")]
        JsError,
        #[source]
        #[cfg(not(target_family = "wasm"))]
        <xwt::current::Connection as xwt_core::datagram::Send>::Error,
    ),

    #[error("failed to read negotiation request")]
    ReadNegotiateRequest(#[source] negotiate::RequestError),
    #[error("failed to read negotiation response")]
    ReadNegotiateResponse(#[source] negotiate::ResponseError),
    #[error("wrong protocol version")]
    WrongProtocolVersion(#[source] negotiate::WrongProtocolVersion),

    #[cfg(not(target_family = "wasm"))]
    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] wtransport::error::ConnectionError),
    #[cfg(not(target_family = "wasm"))]
    #[error("failed to accept session")]
    AcceptSession(#[source] wtransport::error::ConnectionError),

    #[error("TODO")]
    Messages(#[source] message::MessageError),
}
