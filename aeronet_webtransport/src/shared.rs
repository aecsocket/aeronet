use aeronet_proto::packet::MessageSeq;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct MessageKey(MessageSeq);

impl MessageKey {
    #[must_use]
    pub const fn from_raw(seq: MessageSeq) -> Self {
        Self(seq)
    }

    #[must_use]
    pub const fn into_raw(self) -> MessageSeq {
        self.0
    }
}
