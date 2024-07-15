mod backend;
mod frontend;

use std::io;

use aeronet::{
    client::ClientState,
    stats::{MessageStats, Rtt},
};
use aeronet_proto::session::{MtuTooSmall, OutOfMemory, SendError, Session};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use web_time::Duration;

use crate::internal::ConnectionMeta;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        use crate::js_error::JsError;

        pub type ClientConfig = xwt_web_sys::WebTransportOptions;
        type ConnectError = JsError;
        type AwaitConnectError = JsError;
        type ConnectionLostError = JsError;
        type SendDatagramError = JsError;
    } else {
        use std::net::SocketAddr;

        use xwt_core::endpoint::Connect;

        use crate::internal;

        pub type ClientConfig = xwt_wtransport::wtransport::ClientConfig;
        type ConnectError = <internal::ClientEndpoint as Connect>::Error;
        type AwaitConnectError = <<internal::ClientEndpoint as Connect>::Connecting as xwt_core::endpoint::connect::Connecting>::Error;
        type ConnectionLostError = <internal::Connection as xwt_core::session::datagram::Receive>::Error;
        type SendDatagramError = <internal::Connection as xwt_core::session::datagram::Send>::Error;
    }
}

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
    #[error(transparent)]
    Send(SendError),
    #[error(transparent)]
    OutOfMemory(OutOfMemory),

    // backend
    #[error("frontend closed")]
    FrontendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect")]
    Connect(#[source] ConnectError),
    #[error("failed to await connection")]
    AwaitConnect(#[source] AwaitConnectError),
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,
    #[error("connection MTU too small")]
    MtuTooSmall(#[source] MtuTooSmall),
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),

    // connection
    #[error("connection lost")]
    ConnectionLost(#[source] ConnectionLostError),
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
}

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
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<ConnectionMeta>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    session: Session,
}

#[derive(Debug)]
pub struct Connected {
    #[cfg(not(target_family = "wasm"))]
    pub local_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    pub rtt: Duration,
    pub session: Session,
    recv_err: oneshot::Receiver<ClientError>,
    recv_meta: mpsc::Receiver<ConnectionMeta>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
}

impl Rtt for Connected {
    fn rtt(&self) -> Duration {
        self.rtt
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
