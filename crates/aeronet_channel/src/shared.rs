//! Items shared between the client and server.

/// Either side of the channel between the client and server was closed.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("disconnected")]
pub struct Disconnected;

/// Key identifying a message sent from either a [`ChannelClient`] or a
/// [`ChannelServer`].
///
/// This is a pseudo-unique key, since it is unique up until the point where the
/// underlying [`u16`] wraps around.
///
/// [`ChannelClient`]: crate::client::ChannelClient
/// [`ChannelServer`]: crate::server::ChannelServer
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MessageKey(u16);

impl MessageKey {
    /// Creates a new key from its raw sequence value.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    /// Gets the raw sequence value of this key.
    #[must_use]
    pub const fn into_raw(self) -> u16 {
        self.0
    }

    /// Increments this key by one, respecting wraparound.
    pub fn inc(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}
