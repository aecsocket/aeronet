//! Items shared between the client and server.

use aeronet_proto::ty::MessageSeq;

/// Key identifying a message sent across a connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageKey(MessageSeq);

impl MessageKey {
    /// Creates a new key from a raw message sequence.
    #[must_use]
    pub const fn from_raw(seq: MessageSeq) -> Self {
        Self(seq)
    }

    /// Gets the raw message sequence of this key.
    #[must_use]
    pub const fn into_raw(self) -> MessageSeq {
        self.0
    }
}
