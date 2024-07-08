use aeronet_proto::seq::Seq;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct MessageKey(Seq);

impl MessageKey {
    #[must_use]
    pub const fn from_raw(seq: Seq) -> Self {
        Self(seq)
    }

    #[must_use]
    pub const fn into_raw(self) -> Seq {
        self.0
    }
}
