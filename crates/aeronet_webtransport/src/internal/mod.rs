mod backend;
mod frontend;

pub use {backend::*, frontend::*};

use aeronet_proto::session::{FatalSendError, MtuTooSmall, OutOfMemory, SendError, Session};
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};

pub const MSG_BUF_CAP: usize = 256;

// conservative estimate based on
// https://blog.cloudflare.com/increasing-ipv6-mtu/
pub const MIN_MTU: usize = 1024;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type Connection = xwt_web_sys::Session;
        pub type ClientEndpoint = xwt_web_sys::Endpoint;
        pub type ConnectionError = crate::shared::JsError;
    } else {
        use std::net::SocketAddr;

        use web_time::Duration;
        use xwt_core::session::datagram;

        pub type Connection = xwt_wtransport::Connection;
        pub type ClientEndpoint = xwt_wtransport::Endpoint<wtransport::endpoint::endpoint_side::Client>;
        pub type ConnectionError = <Connection as datagram::Receive>::Error;
    }
}

#[derive(Debug)]
pub struct ConnectionMeta {
    // remote addr may change over the lifetime of a connection
    // since QUIC is designed to allow underlying network changes
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    pub rtt: Duration,
    pub mtu: usize,
}

#[derive(Debug)]
pub struct InternalSession {
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    pub raw_rtt: Duration,
    pub session: Session,
    pub recv_meta: mpsc::Receiver<ConnectionMeta>,
    pub send_msgs: mpsc::UnboundedSender<Bytes>,
    pub recv_msgs: mpsc::Receiver<Bytes>,
    pub send_local_dc: oneshot::Sender<String>,
    pub fatal_error: Option<FatalSendError>,
}

// error types
// intentionally don't derive Error so that consumers are forced to map each
// variant to their own error variant

#[derive(Debug)]
pub enum SessionSendError {
    Trivial(SendError),
    Fatal(FatalSendError),
}

#[derive(Debug)]
pub enum SessionError {
    // frontend
    BackendClosed,
    MtuTooSmall(MtuTooSmall),
    OutOfMemory(OutOfMemory),

    // backend
    FrontendClosed,
    DatagramsNotSupported,

    // connection
    ConnectionLost(ConnectionError),
}
