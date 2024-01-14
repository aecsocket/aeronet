use aeronet::MessageStats;

/// Statistics on a connection using a channel transport.
#[derive(Debug, Clone, Default)]
pub struct ConnectionInfo {
    /// Number of messages successfully sent.
    pub msgs_sent: usize,
    /// Number of messages successfully received.
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
