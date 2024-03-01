#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

mod client;
mod server;

pub use {client::*, server::*};

use aeronet::stats::MessageStats;

/// Statistics on a connection using a channel transport.
#[derive(Debug, Clone, Default)]
pub struct ConnectionInfo {
    /// See [`MessageStats::msgs_sent`].
    pub msgs_sent: usize,
    /// See [`MessageStats::msgs_recv`]
    pub msgs_recv: usize,
}

impl MessageStats for ConnectionInfo {
    fn msgs_sent(&self) -> usize {
        self.msgs_sent
    }

    fn msgs_recv(&self) -> usize {
        self.msgs_recv
    }
}

/// Error that occurs when processing a [`ChannelClient`] or [`ChannelServer`].
///
/// [`ChannelClient`]: crate::ChannelClient
/// [`ChannelServer`]: crate::ChannelServer
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The other side is not connected.
    #[error("disconnected")]
    Disconnected,
    /// The client was forcefully disconnected by the app.
    #[error("force disconnect")]
    ForceDisconnect,
    /// The client is already connected to a server.
    #[error("already connected")]
    AlreadyConnected,
    /// The client is already disconnected.
    #[error("already disconnected")]
    AlreadyDisconnected,
}
