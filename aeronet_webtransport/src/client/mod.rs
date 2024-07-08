mod backend;
mod frontend;

use std::{io, net::SocketAddr};

use aeronet::{
    client::ClientState,
    stats::{LocalAddr, MessageStats, RemoteAddr, Rtt},
};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use web_time::Duration;
use xwt_core::endpoint::Connect;

use crate::internal;

#[derive(Derivative, Default)]
#[derivative(Debug = "transparent")]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportClient {
    state: State,
}

type State = ClientState<Connecting, Connected>;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    // frontend
    #[error("backend closed")]
    BackendClosed,
    #[error("already connecting or connected")]
    AlreadyConnected,
    #[error("already disconnected")]
    AlreadyDisconnected,
    #[error("not connected")]
    NotConnected,

    // backend
    #[error("frontend closed")]
    FrontendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect")]
    Connect(#[source] <internal::ClientEndpoint as Connect>::Error),
    #[error("failed to await connection")]
    AwaitConnect(
        #[source]
        <<internal::ClientEndpoint as Connect>::Connecting as xwt_core::endpoint::connect::Connecting>::Error,
    ),
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),

    // connection
    #[error("connection lost")]
    ConnectionLost(#[source] <internal::Connection as xwt_core::session::datagram::Receive>::Error),
    #[error("failed to send datagram")]
    SendDatagram(#[source] <internal::Connection as xwt_core::session::datagram::Send>::Error),
}

#[cfg(not(target_family = "wasm"))]
pub type ClientConfig = xwt_wtransport::wtransport::ClientConfig;

#[cfg(target_family = "wasm")]
pub type ClientConfig = xwt_web_sys::WebTransportOptions;

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
    initial_rtt: Duration,
    recv_rtt: mpsc::Receiver<Duration>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
}

#[derive(Debug)]
pub struct Connected {
    #[cfg(not(target_family = "wasm"))]
    pub local_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    pub rtt: Duration,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
    recv_err: oneshot::Receiver<ClientError>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
}

impl Rtt for Connected {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

#[cfg(not(target_family = "wasm"))]
impl LocalAddr for Connected {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

#[cfg(not(target_family = "wasm"))]
impl RemoteAddr for Connected {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}
