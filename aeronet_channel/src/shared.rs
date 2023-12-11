slotmap::new_key_type! {
    /// Key type used to uniquely identify a client connected to a
    /// [`ChannelServer`].
    ///
    /// [`ChannelServer`]: crate::ChannelServer
    pub struct ClientKey;
}

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
    /// This client is already connected to a server.
    #[error("already connected")]
    Connected,
}
