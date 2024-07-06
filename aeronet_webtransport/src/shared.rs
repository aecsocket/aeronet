use aeronet_proto::seq::Seq;
use web_time::Duration;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionStats {
    pub rtt: Duration,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}
