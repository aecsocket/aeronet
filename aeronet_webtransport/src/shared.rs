use aeronet::stats::{MessageStats, Rtt};
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

impl Rtt for ConnectionStats {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl MessageStats for ConnectionStats {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}
