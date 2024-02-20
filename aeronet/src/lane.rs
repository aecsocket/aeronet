use std::fmt::Debug;

use crate::TransportProtocol;

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

#[derive(Debug, Clone)]
pub struct LaneConfig {
    pub kind: LaneKind,
}

/// App-defined type listing a set of lanes which a transport can use to send
/// app messages along.
///
/// See [`LaneKind`] for an explanation of lanes.
///
/// This trait should be derived - see [`aeronet_derive::LaneKey`]. Otherwise,
/// you will have to make sure to follow the contract regarding panics.
///
/// # Panic safety
///
/// This trait must be implemented correctly, otherwise transport
/// implementations may panic.
pub trait LaneKey: Send + Sync + Debug + Clone + Copy + 'static {
    /// All variants of this type that may exist.
    ///
    /// # Panic safety
    ///
    /// This must contain every possible value that may exist, otherwise
    /// transport implementations may panic.
    const VARIANTS: &'static [Self];

    fn config() -> Vec<LaneConfig> {
        Self::VARIANTS
            .iter()
            .map(|variant| LaneConfig {
                kind: variant.kind(),
            })
            .collect()
    }

    /// Index of this value in the [`LaneKey::VARIANTS`] array.
    ///
    /// # Panic safety
    ///
    /// This must be a valid index in the variants array, meaning:
    /// * it is not out of the bounds of the array
    /// * the value in the variants array at this index is identical to `self`
    fn index(&self) -> usize;

    /// What kind of lane this value represents.
    fn kind(&self) -> LaneKind;
}

/// Defines what lane a [`Message`] is sent on.
///
/// See [`LaneKind`] for an explanation of lanes.
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

/// Defines what type of [`LaneKey`] that [`Message`]s are sent over.
///
/// Transports may send messages on different [lanes](LaneKey), and need a way
/// to determine:
/// * What are all of the possible lanes available to send messages on?
///   * For example, if a transport needs to set up lanes in advance, it needs
///     to know all of the possible lanes beforehand.
/// * What specific lane is this specific message sent on?
///
/// This trait allows the user to specify which user-defined type, implementing
/// [`LaneKey`], is used for these functions.
pub trait LaneProtocol: TransportProtocol {
    /// User-defined type of lane that the transport uses.
    type Lane: LaneKey;
}
