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
        pub type ConnectionError = crate::JsError;

        // TODO upstreamed to xwt
        #[allow(clippy::unnecessary_wraps)] // must match fn sig
        pub fn get_mtu(conn: &Connection) -> Option<usize> {
            let mtu = usize::try_from(conn.transport.datagrams().max_datagram_size())
                .expect("should be able to fit u32 into usize");
            Some(mtu)
        }
    } else {
        use std::net::SocketAddr;

        use web_time::Duration;
        use xwt_core::session::datagram;

        pub type Connection = xwt_wtransport::Connection;
        pub type ClientEndpoint = xwt_wtransport::Endpoint<wtransport::endpoint::endpoint_side::Client>;
        pub type ConnectionError = <Connection as datagram::Receive>::Error;

        // TODO upstreamed to xwt
        pub fn get_mtu(conn: &Connection) -> Option<usize> {
            conn.0.max_datagram_size()
        }
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
pub struct ConnectionInner<E> {
    #[cfg(not(target_family = "wasm"))]
    pub remote_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    pub raw_rtt: Duration,
    pub session: Session,
    pub recv_err: oneshot::Receiver<E>,
    pub recv_meta: mpsc::Receiver<ConnectionMeta>,
    pub send_msgs: mpsc::UnboundedSender<Bytes>,
    pub recv_msgs: mpsc::Receiver<Bytes>,
    pub send_local_dc: oneshot::Sender<String>,
    pub recv_remote_dc: oneshot::Receiver<String>,
    pub fatal_error: Option<FatalSendError>,
}

// intentionally don't derive Error so that consumers are forced to map each
// variant to their own error variant
#[derive(Debug)]
pub enum InternalError<E> {
    Spec(E),

    // frontend
    BackendClosed,
    MtuTooSmall(MtuTooSmall),
    OutOfMemory(OutOfMemory),
    Send(SendError),
    FatalSend(FatalSendError),

    // backend
    FrontendClosed,
    DatagramsNotSupported,

    // connection
    ConnectionLost(ConnectionError),
}
