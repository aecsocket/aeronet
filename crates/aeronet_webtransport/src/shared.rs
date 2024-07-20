//! Items shared between the client and server.

use aeronet::lane::LaneIndex;
use aeronet_proto::ty::MessageSeq;

/// Key identifying a message sent across a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct MessageKey {
    lane: LaneIndex,
    seq: MessageSeq,
}

impl MessageKey {
    /// Creates a new key from its raw parts.
    #[must_use]
    pub const fn from_raw(lane: LaneIndex, seq: MessageSeq) -> Self {
        Self { lane, seq }
    }

    /// Gets the raw parts of this key.
    #[must_use]
    pub const fn into_raw(self) -> (LaneIndex, MessageSeq) {
        (self.lane, self.seq)
    }
}
