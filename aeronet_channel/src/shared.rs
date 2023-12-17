use crate::ClientKey;

/// Error that occurs when processing a [`ChannelClient`] or [`ChannelServer`].
///
/// [`ChannelClient`]: crate::ChannelClient
/// [`ChannelServer`]: crate::ChannelServer
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// A client with the given key does not exist.
    #[error("no client with key {0:?}")]
    NoClient(ClientKey),
    /// The other side disconnected from this side, due to the other side being
    /// dropped and closing the MPSC channels.
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
