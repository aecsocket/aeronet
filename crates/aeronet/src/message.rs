//! Types describing individual messages which may be sent and received at the
//! [transport layer].
//!
//! [transport layer]: crate::transport

use {
    arbitrary::Arbitrary,
    bevy_reflect::prelude::*,
    datasize::DataSize,
    derive_more::{Add, AddAssign, Sub, SubAssign},
    std::{cmp::Ordering, num::Wrapping},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Arbitrary, DataSize)]
pub enum SendMode {
    UnreliableUnordered,
    UnreliableSequenced,
    ReliableUnordered,
    ReliableOrdered(u32),
}

impl SendMode {
    /// Gets the reliability of this send mode.
    #[must_use]
    pub const fn reliability(&self) -> SendReliability {
        match self {
            Self::UnreliableUnordered | Self::UnreliableSequenced => SendReliability::Unreliable,
            Self::ReliableUnordered | Self::ReliableOrdered(_) => SendReliability::Reliable,
        }
    }
}

/// Reliability of a [`SendMode`].
///
/// See [`SendMode`] for more info on reliability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Arbitrary, DataSize)]
pub enum SendReliability {
    /// Messages are not guaranteed to be delivered.
    Unreliable,
    /// Messages are guaranteed to be delivered.
    Reliable,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    Reflect,
    Arbitrary,
    DataSize,
    Add,
    AddAssign,
    Sub,
    SubAssign,
)]
pub struct Seq(#[data_size(skip)] Wrapping<u16>);

impl Seq {
    /// Creates a [`Seq`] from a raw sequence number.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self(Wrapping(raw))
    }

    /// Gets the raw sequence number.
    #[must_use]
    pub const fn into_raw(self) -> u16 {
        self.0.0
    }
}

impl Ord for Seq {
    /// Logically compares `self` to `other` in a way that respects wrap-around
    /// of sequence numbers, treating e.g. `0 cmp 1` as [`Less`] (as expected),
    /// but `0 cmp 65535` as [`Greater`].
    ///
    /// See [*Gaffer On Games*].
    ///
    /// If the two values compared have a real difference equal to or larger
    /// than `u16::MAX / 2`, no guarantees are upheld.
    ///
    /// [`Greater`]: Ordering::Greater
    /// [`Less`]: Ordering::Less
    ///
    /// [*Gaffer On Games*]: https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/#handling-sequence-number-wrap-around
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        // The implementation used is a variant of `slotmap`'s generation
        // comparison function:
        // https://github.com/orlp/slotmap/blob/c905b6c/src/util.rs#L10
        // It has been adapted to use u16s and Ordering.
        // This is used instead of the Gaffer On Games code because it produces
        // smaller assembly, but has a tiny difference in behaviour around
        // `u16::MAX / 2`.

        let s1 = self.into_raw();
        let s2 = other.into_raw();
        (s1 as i16).wrapping_sub(s2 as i16).cmp(&0)
    }
}

impl PartialOrd for Seq {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pseudo-unique key for a [transport layer] message that has been sent out by
/// us, used for detecting when the peer sent acknowledgement of this message.
///
/// The underlying [`Seq`] should be treated as an opaque value, specific to the
/// transport layer implementation.
///
/// # Uniqueness
///
/// The underlying type is a [`Seq`], which may overflow during the lifetime of
/// the session. Uniqueness is only guaranteed up until the overflow, so you
/// should not store [`MessageKey`]s for a long time (around [RTT] plus a safety
/// margin).
///
/// [transport layer]: crate::transport
/// [RTT]: crate::rtt
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Reflect, Arbitrary, DataSize,
)]
pub struct MessageKey(pub Seq);
