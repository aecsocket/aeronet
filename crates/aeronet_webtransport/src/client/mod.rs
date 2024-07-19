//! Server-side transport implementation.

mod backend;
mod frontend;

use std::io;

use aeronet::{
    client::ClientState,
    stats::{ConnectedAt, MessageStats, Rtt},
};
use aeronet_proto::session::{MtuTooSmall, OutOfMemory, SendError, Session};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use web_time::{Duration, Instant};

use crate::internal::ConnectionMeta;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        use crate::js_error::JsError;

        /// Client network configuration.
        pub type ClientConfig = xwt_web_sys::WebTransportOptions;
        type ConnectError = JsError;
        type AwaitConnectError = JsError;
        type ConnectionLostError = JsError;
    } else {
        use std::net::SocketAddr;

        use xwt_core::endpoint::Connect;

        use crate::internal;

        /// Client network configuration.
        pub type ClientConfig = xwt_wtransport::wtransport::ClientConfig;
        type ConnectError = <internal::ClientEndpoint as Connect>::Error;
        type AwaitConnectError = <<internal::ClientEndpoint as Connect>::Connecting as xwt_core::endpoint::connect::Connecting>::Error;
        type ConnectionLostError = <internal::Connection as xwt_core::session::datagram::Receive>::Error;
    }
}

/// WebTransport implementation of [`ClientTransport`].
///
/// See the [crate-level documentation](crate).
///
/// [`ClientTransport`]: aeronet::client::ClientTransport
#[derive(Derivative, Default)]
#[derivative(Debug = "transparent")]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportClient {
    state: State,
}

type State = ClientState<Connecting, Connected>;

/// Error type for operations on a [`WebTransportClient`].
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    // frontend
    /// Backend client task was cancelled, dropping the underlying connection.
    #[error("backend closed")]
    BackendClosed,
    /// Client is already connecting or connected.
    #[error("already connecting or connected")]
    AlreadyConnected,
    /// Client is already disconnected.
    #[error("already disconnected")]
    AlreadyDisconnected,
    /// Client is not connected.
    #[error("not connected")]
    NotConnected,
    /// See [`SendError`].
    #[error(transparent)]
    Send(SendError),
    /// See [`OutOfMemory`].
    #[error(transparent)]
    OutOfMemory(OutOfMemory),

    // backend
    /// Client frontend was closed.
    #[error("frontend closed")]
    FrontendClosed,
    /// Failed to create the endpoint to run the connection on.
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    /// Failed to connect to the target.
    #[error("failed to connect")]
    Connect(#[source] ConnectError),
    /// Failed to await the connection to the target.
    #[error("failed to await connection")]
    AwaitConnect(#[source] AwaitConnectError),
    /// Established a connection with the server, but it does not support
    /// datagrams.
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,
    /// Server supports datagrams, but the maximum datagram size it supports is
    /// too small for us.
    #[error("connection MTU too small")]
    MtuTooSmall(#[source] MtuTooSmall),
    /// Frontend forced a disconnect from the server.
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),

    // connection
    /// Lost connection.
    #[error("connection lost")]
    ConnectionLost(#[source] ConnectionLostError),
}

/// State of a [`WebTransportClient`] when it is [`ClientState::Connecting`].
#[derive(Debug)]
pub struct Connecting {
    recv_connected: oneshot::Receiver<ToConnected>,
    recv_err: oneshot::Receiver<ClientError>,
}

#[derive(Debug)]
struct ToConnected {
    #[cfg(not(target_family = "wasm"))]
    local_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    remote_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_rtt: Duration,
    recv_meta: mpsc::Receiver<ConnectionMeta>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    session: Session,
}

/// State of a [`WebTransportClient`] when it is [`ClientState::Connected`].
#[derive(Debug)]
pub struct Connected {
    /// See [`LocalAddr`].
    #[cfg(not(target_family = "wasm"))]
    pub local_addr: SocketAddr,
    /// See [`RemoteAddr`].
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    /// Backing [`Rtt`] value provided by the [`wtransport`] connection.
    ///
    /// The [`Rtt`] impl for this struct returns the [`Session`]'s RTT, *not*
    /// this value. This value is more representative of RTT at a packet level,
    /// but less representative of RTT at the application level.
    #[cfg(not(target_family = "wasm"))]
    pub raw_rtt: Duration,
    /// Protocol session state, used for reading more advanced info.
    pub session: Session,
    recv_err: oneshot::Receiver<ClientError>,
    recv_meta: mpsc::Receiver<ConnectionMeta>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
}

impl ConnectedAt for Connected {
    fn connected_at(&self) -> Instant {
        self.session.connected_at()
    }
}

impl Rtt for Connected {
    fn rtt(&self) -> Duration {
        self.session.rtt().get()
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.session.bytes_sent()
    }

    fn bytes_recv(&self) -> usize {
        self.session.bytes_recv()
    }
}

#[cfg(not(target_family = "wasm"))]
impl aeronet::stats::LocalAddr for Connected {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

#[cfg(not(target_family = "wasm"))]
impl aeronet::stats::RemoteAddr for Connected {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}
