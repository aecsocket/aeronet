use std::cmp;

use bitcode::{Decode, Encode};

/// Sequence number uniquely identifying a message sent across a network.
///
/// Note that the sequence number identifies a *message*, not anything else like
/// a packet or fragment.
///
/// The number is stored internally as a [`u16`], which means it will wrap
/// around fairly quickly as many messages can be sent per second. Users of a
/// sequence number should take this into account, and use the custom
/// [`Seq::partial_cmp`] implementation which takes wraparound into
/// consideration.
///
/// See <https://gafferongames.com/post/packet_fragmentation_and_reassembly/>, *Fragment Packet Structure*.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Ord, Hash, Encode, Decode)]
pub struct Seq(pub u16);

impl Seq {
    /// Returns the current sequence value and increments `self`.
    pub fn next(&mut self) -> Seq {
        let cur = *self;
        self.0 = self.0.wrapping_add(1);
        cur
    }
}

impl cmp::PartialOrd for Seq {
    /// Logically compares `self` to `other` in a way that respects wrap-around
    /// of sequence numbers, treating e.g. `65535 cmp 0` as [`Greater`], but
    /// `1 cmp 0` as [`Less`].
    ///
    /// See <https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/>,
    /// *Handling Sequence Number Wrap-Around*.
    ///
    /// [`Greater`]: cmp::Ordering::Greater
    /// [`Less`]: cmp::Ordering::Less
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        const HALF: u16 = u16::MAX / 2;

        let s1 = self.0;
        let s2 = other.0;

        if s1 == s2 {
            return Some(cmp::Ordering::Equal);
        }

        if ((s1 > s2) && (s1 - s2 <= HALF)) || ((s1 < s2) && (s2 - s1 > HALF)) {
            Some(cmp::Ordering::Greater)
        } else {
            Some(cmp::Ordering::Less)
        }
    }
}
