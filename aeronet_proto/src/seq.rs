//! See [`Seq`].

use std::{
    cmp::Ordering,
    convert::Infallible,
    ops::{Add, AddAssign, Sub, SubAssign},
};

use arbitrary::Arbitrary;
use octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write};

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
/// # Wraparound
///
/// Operations on [`Seq`] must take into account wraparound, as it is inevitable
/// that it will eventually occur in the program - a [`u16`] is relatively very
/// small.
///
/// The sequence number can be visualized as an infinite number line, where
/// [`u16::MAX`] is right before `0`, `0` is before `1`, etc.:
///
/// ```text
///     65534  65535    0      1      2
/// ... --|------|------|------|------|-- ...
/// ```
///
/// [Addition](std::ops::Add) and [subtraction](std::ops::Sub) will always wrap.
///
/// See <https://gafferongames.com/post/packet_fragmentation_and_reassembly/>, *Fragment Packet Structure*.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Arbitrary)]
pub struct Seq(pub u16);

impl Seq {
    /// Sequence number with value [`u16::MAX`].
    pub const MAX: Seq = Seq(u16::MAX);

    /// Gets a signed number for the value of packet sequences "elapsed" between
    /// `rhs` and `self`.
    ///
    /// This is effectively `rhs - self`, but taking into account wraparound and
    /// therefore returning a signed value. This will always return the smallest
    /// path around this "circle".
    ///
    /// ```text
    ///     65534  65535    0      1      2
    /// ... --|------|------|------|------|-- ...
    ///       ^             ^      ^      ^
    ///       |             +------+------+ 0.dist_to(2) = 2
    ///       |                    |        2.dist_to(0) = -2
    ///       +--------------------+ 65534.dist_to(1) = 3
    ///                              1.dist_to(65534) = -3
    /// ```
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::seq::Seq;
    /// assert_eq!(Seq(0).dist_to(Seq(0)), 0);
    /// assert_eq!(Seq(0).dist_to(Seq(5)), 5);
    /// assert_eq!(Seq(3).dist_to(Seq(5)), 2);
    /// assert_eq!(Seq(1).dist_to(Seq(0)), -1);
    /// assert_eq!(Seq(2).dist_to(Seq(0)), -2);
    ///
    /// assert_eq!(Seq(0).dist_to(Seq::MAX), -1);
    /// assert_eq!(Seq::MAX.dist_to(Seq::MAX), 0);
    ///
    /// assert_eq!(Seq::MAX.dist_to(Seq(0)), 1);
    /// assert_eq!((Seq::MAX - Seq(3)).dist_to(Seq(0)), 4);
    ///
    /// assert_eq!(Seq::MAX.dist_to(Seq(3)), 4);
    /// assert_eq!((Seq::MAX - Seq(3)).dist_to(Seq(3)), 7);
    /// ```
    #[must_use]
    pub const fn dist_to(self, rhs: Self) -> i16 {
        #[allow(clippy::cast_possible_wrap)] // that's exactly what we want
        (rhs.0.wrapping_sub(self.0) as i16)
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

impl Add<Seq> for Seq {
    type Output = Seq;

    fn add(self, rhs: Seq) -> Self::Output {
        Self(self.0.wrapping_add(rhs.0))
    }
}

impl AddAssign for Seq {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub<Seq> for Seq {
    type Output = Seq;

    fn sub(self, rhs: Seq) -> Self::Output {
        Self(self.0.wrapping_sub(rhs.0))
    }
}

impl SubAssign for Seq {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl FixedEncodeLen for Seq {
    const ENCODE_LEN: usize = u16::ENCODE_LEN;
}

impl Encode for Seq {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.0)
    }
}

impl Decode for Seq {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self(src.read()?))
    }
}

#[cfg(test)]
mod tests {
    use octs::BytesMut;

    use super::*;

    #[test]
    fn encode_decode() {
        let v = Seq(1234);
        let mut buf = BytesMut::with_capacity(Seq::ENCODE_LEN);

        buf.write(&v).unwrap();
        assert_eq!(Seq::ENCODE_LEN, buf.len());

        assert_eq!(v, buf.freeze().read::<Seq>().unwrap());
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
