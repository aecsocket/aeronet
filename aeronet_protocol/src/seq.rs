use std::cmp;

use arbitrary::Arbitrary;

/// Sequence number uniquely identifying a message sent across a network.
///
/// Note that the sequence number identifies a *message*, not anything else like
/// a packet or fragment.
///
/// The number is stored internally as a [`u16`], which means it will wrap
/// around fairly quickly as many messages can be sent per second. Users of a
/// sequence number should take this into account, and use the custom
/// [`Seq::cmp`] implementation which takes wraparound into
/// consideration.
///
/// See <https://gafferongames.com/post/packet_fragmentation_and_reassembly/>, *Fragment Packet Structure*.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary)]
pub struct Seq(pub u16);

impl Seq {
    pub fn encode(&self, buf: &mut octets::OctetsMut<'_>) -> octets::Result<()> {
        buf.put_u16(self.0)?;
        Ok(())
    }

    pub fn decode(buf: &mut octets::Octets<'_>) -> octets::Result<Self> {
        buf.get_u16().map(Self)
    }

    /// Returns the current sequence value and increments `self`.
    #[must_use]
    pub fn get_and_increment(&mut self) -> Self {
        let cur = *self;
        self.0 = self.0.wrapping_add(1);
        cur
    }
}

impl cmp::Ord for Seq {
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
    /// [`Greater`]: cmp::Ordering::Greater
    /// [`Less`]: cmp::Ordering::Less
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // The implementation used is a variant of `slotmap`'s generation
        // comparison function:
        // https://github.com/orlp/slotmap/blob/c905b6ced490551476cb7c37778eb8128bdea7ba/src/util.rs#L10
        // It has been adapted to use u16s and Ordering.
        // This is used instead of the Gaffer On Games code because it produces
        // smaller assembly, but has a tiny difference in behaviour around `u16::MAX /
        // 2`.

        let s1 = self.0;
        let s2 = other.0;

        // alternate impl
        // s1.wrapping_add(HALF.wrapping_sub(s2)).cmp(&(u16::MAX / 2))
        #[allow(clippy::cast_possible_wrap)] // that's exactly what we want
        (s1 as i16).wrapping_sub(s2 as i16).cmp(&0)
    }
}

impl cmp::PartialOrd for Seq {
    /// See [`Seq::cmp`].
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increasing_wraparound() {
        assert!(Seq(0) < Seq(1));
        assert!(Seq(1) < Seq(2));
        assert!(Seq(u16::MAX - 3) < Seq(u16::MAX));
        assert!(Seq(u16::MAX - 2) < Seq(u16::MAX));
        assert!(Seq(u16::MAX - 1) < Seq(u16::MAX));

        assert!(Seq(u16::MAX) < Seq(0));
        assert!(Seq(u16::MAX) < Seq(1));
        assert!(Seq(u16::MAX) < Seq(2));

        assert!(Seq(u16::MAX - 3) < Seq(2));

        // NOTE: we explicitly don't test what happens when the difference
        // is around u16::MAX, because we guarantee no behaviour there
        // that's like saying that a packet arrived after 32,000 other packets;
        // if that happens, then we're kinda screwed anyway
        // we also don't test decreasing wraparound because that won't happen
        // in our use-case
    }
}
