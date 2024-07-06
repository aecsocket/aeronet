mod backend;
mod frontend;

use std::{io, net::SocketAddr};

use aeronet::{
    client::ClientState,
    stats::{LocalAddr, MessageStats, RemoteAddr, Rtt},
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use octs::Bytes;
use xwt_core::endpoint::Connect;

use crate::shared::ConnectionStats;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type ClientConfig = ();
        type Endpoint = xwt::current::Endpoint;
    } else {
        pub type ClientConfig = wtransport::ClientConfig;
        type Endpoint = xwt::current::Endpoint<wtransport::endpoint::endpoint_side::Client>;
    }
}

#[derive(Derivative)]
#[derivative(Debug = "transparent")]
pub struct WebTransportClient {
    state: ClientState<Connecting, Connected>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    // frontend
    #[error("frontend closed")]
    FrontendClosed,
    #[error("already connecting or connected")]
    AlreadyConnected,
    #[error("already disconnected")]
    AlreadyDisconnected,

    // backend
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect")]
    Connect(#[source] <Endpoint as Connect>::Error),
    #[error("failed to await connection")]
    AwaitConnect(
        #[source]
        <<Endpoint as Connect>::Connecting as xwt_core::endpoint::connect::Connecting>::Error,
    ),
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),
}

#[derive(Debug)]
pub struct Connecting {
    recv_connected: oneshot::Receiver<Connected>,
    recv_err: oneshot::Receiver<ClientError>,
}

#[derive(Debug)]
pub struct Connected {
    #[cfg(not(target_family = "wasm"))]
    pub local_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    pub stats: ConnectionStats,
    recv_s2c: mpsc::Receiver<Bytes>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
}

impl Rtt for Connected {
    fn rtt(&self) -> web_time::Duration {
        self.stats.rtt
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.stats.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.stats.bytes_recv
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
