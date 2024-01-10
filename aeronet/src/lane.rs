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

/// App-defined type listing a set of lanes which a transport can use to send
/// app messages along.
/// 
/// See [`LaneKind`] for documentation on lanes.
/// 
/// This trait should be derived - see [`aeronet_derive::LaneKey`]. Otherwise,
/// you will have to make sure to follow the contract regarding panics.
/// 
/// # Panics
/// 
/// This trait must be implemented correctly, otherwise transport
/// implementations may panic.
pub trait LaneKey: Send + Sync + Debug + Clone + 'static {
    /// All variants of this type that may exist.
    /// 
    /// # Panics
    /// 
    /// This must contain every possible value that may exist, otherwise
    /// transport implementations may panic.
    const VARIANTS: &'static [Self];

    /// Index of this value in the [`LaneKey::VARIANTS`] array.
    /// 
    /// # Panics
    /// 
    /// This must be a valid index in the variants array, meaning:
    /// * it is not out of the bounds of the array
    /// * the value in the variants array at this index is identical to `self`
    fn variant(&self) -> usize;

    /// What kind of lane this value represents.
    fn kind(&self) -> LaneKind;

    /// Relative priority of this lane compared to other lanes.
    ///
    /// When bandwidth is limited, lanes with a higher priority will have their
    /// buffered messages sent out sooner.
    /// 
    /// This value is implementation-specific - some transports may choose to
    /// respect this value; for others, it may have no effect.
    fn priority(&self) -> i32;
}

/// Defines what lane a [`Message`] is sent on.
/// 
/// See [`LaneKey`] for an explanation of lanes.
/// 
/// This trait can be derived - see [`aeronet_derive::OnLane`].
///
/// Note that this only affects what lane an *outgoing* message is *sent out*
/// on - it has no effect on incoming messages.
/// 
/// [`Message`]: crate::Message
pub trait OnLane {
    /// User-defined type of lane, output by [`OnLane::lane`].
    type Lane: LaneKey;

    /// What lane this value is sent out on.
    fn lane(&self) -> Self::Lane;
}
