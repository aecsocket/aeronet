/// The capacity of the buffer used by the message channels.
pub(super) const MSG_BUF_CAP: usize = 16;

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
