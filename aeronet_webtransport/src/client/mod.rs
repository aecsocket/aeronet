mod backend;
mod frontend;

use std::{io, net::SocketAddr};

use aeronet::{
    client::ClientState,
    stats::{LocalAddr, RemoteAddr},
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use octs::Bytes;

use crate::shared::ConnectionStats;

#[derive(Derivative)]
#[derivative(Debug = "transparent")]
pub struct WebTransportClient {
    state: ClientState<Connecting, Connected>,
}

#[cfg(target_family = "wasm")]
pub type ClientConfig = ();

#[cfg(not(target_family = "wasm"))]
pub type ClientConfig = wtransport::ClientConfig;

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
    Connect(#[source] io::Error),
    #[error("failed to await connection")]
    AwaitConnect(#[source] io::Error),
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,
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
    pub remote_addr: SocketAddr,
    pub stats: ConnectionStats,
    recv_s2c: mpsc::Receiver<Bytes>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
}

impl LocalAddr for Connected {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl RemoteAddr for Connected {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}
