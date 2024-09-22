//! Allows creating a dedicated client session, which connects to a server
//! endpoint.
//!
//! See [`WebTransportClient`].

mod backend;
mod frontend;

pub use frontend::*;
use {
    crate::session::{SessionError, SessionMeta},
    bytes::Bytes,
    futures::channel::{mpsc, oneshot},
    thiserror::Error,
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        /// Configuration for the [`WebTransportClient`] on WASM platforms.
        pub type ClientConfig = xwt_web_sys::WebTransportOptions;

        type ConnectError = crate::JsError;
        type AwaitConnectError = crate::JsError;
    } else {
        use wtransport::endpoint::endpoint_side;
        use xwt_core::endpoint::{Connect, connect::Connecting};

        /// Configuration for the [`WebTransportClient`] on non-WASM platforms.
        pub type ClientConfig = wtransport::ClientConfig;
        type ClientEndpoint = xwt_wtransport::Endpoint<endpoint_side::Client>;

        type ConnectError = <ClientEndpoint as Connect>::Error;
        type AwaitConnectError = <<ClientEndpoint as Connect>::Connecting as Connecting>::Error;
    }
}

/// [`WebTransportClient`] error.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Failed to start connecting to the target.
    #[error("failed to connect")]
    Connect(#[source] ConnectError),
    /// Failed to await the connection to the target.
    #[error("failed to await connection")]
    AwaitConnect(#[source] AwaitConnectError),
    /// Generic session error.
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug)]
struct ToConnected {
    #[cfg(not(target_family = "wasm"))]
    local_addr: std::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_remote_addr: std::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_rtt: std::time::Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::Receiver<Bytes>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}
