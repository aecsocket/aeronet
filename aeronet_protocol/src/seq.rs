use std::cmp::Ordering;

use arbitrary::Arbitrary;
use bytes::{BufMut, Bytes, BytesMut};
use safer_bytes::SafeBuf;

use crate::bytes::ReadError;

/// Sequence number uniquely identifying an item sent across a network.
///
/// Note that the sequence number may identify either a message or a packet
/// sequence number.
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
    /// [Encoded] size of this value in bytes.
    ///
    /// [Encoded]: Seq::encode
    pub const ENCODE_SIZE: usize = std::mem::size_of::<u16>();

    /// Encodes this value into a byte buffer.
    ///
    /// The buffer should have at least [`ENCODE_SIZE`] bytes of capacity, to
    /// not have to allocate more space.
    ///
    /// [`ENCODE_SIZE`]: Seq::ENCODE_SIZE
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u16(self.0);
    }

    /// Decodes this value from a byte buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is shorter than [`ENCODE_SIZE`].
    ///
    /// [`ENCODE_SIZE`]: Seq::ENCODE_SIZE
    pub fn decode(buf: &mut Bytes) -> Result<Self, ReadError> {
        let seq = buf.try_get_u16()?;
        Ok(Self(seq))
    }

    /// Returns the current sequence value and increments `self`.
    #[must_use]
    pub fn get_inc(&mut self) -> Self {
        let cur = *self;
        self.0 = self.0.wrapping_add(1);
        cur
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
        // https://github.com/orlp/slotmap/blob/c905b6ced490551476cb7c37778eb8128bdea7ba/src/util.rs#L10
        // It has been adapted to use u16s and Ordering.
        // This is used instead of the Gaffer On Games code because it produces
        // smaller assembly, but has a tiny difference in behaviour around
        // `u16::MAX / 2`.

        let s1 = self.0;
        let s2 = other.0;

        // alternate impl
        // s1.wrapping_add(HALF.wrapping_sub(s2)).cmp(&(u16::MAX / 2))
        #[allow(clippy::cast_possible_wrap)] // that's exactly what we want
        (s1 as i16).wrapping_sub(s2 as i16).cmp(&0)
    }
}

impl PartialOrd for Seq {
    /// See [`Seq::cmp`].
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode() {
        let seq = Seq(1234);
        let mut buf = BytesMut::with_capacity(Seq::ENCODE_SIZE);

        seq.encode(&mut buf);
        assert_eq!(Seq::ENCODE_SIZE, buf.len());

        assert_eq!(seq, Seq::decode(&mut Bytes::from(buf.to_vec())).unwrap());
    }

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
