use std::fmt::Debug;

// todo docs
/// | [`LaneKind`]              | Fragmentation | Reliability | Sequencing |
/// |---------------------------|---------------|-------------|------------|
/// | [`UnreliableUnsequenced`] | ✅            |              |            |
/// | [`UnreliableSequenced`]   | ✅            |              | (1)        |
/// | [`ReliableUnordered`]     | ✅            | ✅            |            |
/// | [`ReliableOrdered`]       | ✅            | ✅            | (2)        |
///
/// 1. If delivery of a single chunk fails, delivery of the whole packet fails
///    (unreliable). If the message arrives later than a message sent and
///    received previously, the message is discarded (sequenced, not ordered).
/// 2. If delivery of a single chunk fails, delivery of all messages halts until
///    that single chunk is received (reliable ordered).
///
/// [`UnreliableUnsequenced`]: LaneKind::UnreliableUnsequenced
/// [`UnreliableSequenced`]: LaneKind::UnreliableSequenced
/// [`ReliableUnordered`]: LaneKind::ReliableUnordered
/// [`ReliableOrdered`]: LaneKind::ReliableOrdered
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneKind {
    UnreliableUnsequenced,
    UnreliableSequenced,
    ReliableUnordered,
    ReliableOrdered,
}

pub trait LaneKey: Send + Sync + Debug + Clone + 'static {
    const VARIANTS: &'static [Self];

    fn variant(&self) -> usize;

    fn kind(&self) -> LaneKind;

    /// Relative priority of this lane compared to other lanes.
    ///
    /// When bandwidth is limited, lanes with a higher priority will have their
    /// buffered messages sent out first. This value is implementation-specific.
    fn priority(&self) -> i32;
}

pub trait OnLane {
    type Lane: LaneKey;

    fn lane(&self) -> Self::Lane;
}
