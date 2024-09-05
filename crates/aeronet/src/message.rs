//! Types describing individual messages which may be sent and received at the
//! [transport layer].
//!
//! [transport layer]: crate::transport

use std::{cmp::Ordering, num::Wrapping};

use arbitrary::Arbitrary;
use bevy_reflect::prelude::*;
use datasize::DataSize;
use derive_more::{Add, AddAssign, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Arbitrary, DataSize)]
pub enum SendMode {
    UnreliableUnordered,
    UnreliableSequenced,
    ReliableUnordered,
    ReliableOrdered(usize),
}

impl SendMode {
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

/// Ordering of a [`SendMode`].
///
/// See [`SendMode`] for more info on ordering.
pub enum SendOrdering {
    /// Messages have no guarantees on ordering, and duplicates may be received.
    Unordered,
    /// Messages will be received in the order they are sent, however if a
    /// message is received out of order, it will be discarded.
    ///
    /// For example, if messages A and B are sent in that order, and the
    /// receiver receives B then A, B will be received and A will be discarded
    /// as it is older than the latest received message (B). Duplicates will not
    /// be received.
    Sequenced,
    /// Messages will be received in the order they are sent, with no gaps.
    Ordered,
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
    pub const ZERO: Seq = Seq::from_raw(0);

    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self(Wrapping(raw))
    }

    #[must_use]
    pub const fn into_raw(self) -> u16 {
        self.0 .0
    }
}

impl Ord for Seq {
    /// Logically compares `self` to `other` in a way that respects wrap-around
    /// of sequence numbers, treating e.g. `0 cmp 1` as [`Less`] (as expected),
    /// but `0 cmp 65535` as [`Greater`].
    ///
    /// See <https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/>,
    /// *Handling Sequence Number Wrap-Around*.
    ///
    /// If the two values compared have a real difference equal to or larger
    /// than `u16::MAX / 2`, no guarantees are upheld.
    ///
    /// [`Greater`]: Ordering::Greater
    /// [`Less`]: Ordering::Less
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

        // alternate impl
        // s1.wrapping_add(HALF.wrapping_sub(s2)).cmp(&(u16::MAX / 2))
        (s1 as i16).wrapping_sub(s2 as i16).cmp(&0)
    }
}

impl PartialOrd for Seq {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Arbitrary, DataSize)]
pub struct MessageKey(pub Seq);
