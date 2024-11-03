use {
    crate::packet::{MessageSeq, PacketSeq, Seq},
    octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write},
    std::{
        cmp::Ordering,
        convert::Infallible,
        fmt,
        ops::{Add, AddAssign, Sub, SubAssign},
    },
};

impl Seq {
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
    /// # use aeronet_transport::packet::Seq;
    /// assert_eq!(Seq(0).dist_to(Seq(0)), 0);
    /// assert_eq!(Seq(0).dist_to(Seq(5)), 5);
    /// assert_eq!(Seq(3).dist_to(Seq(5)), 2);
    /// assert_eq!(Seq(1).dist_to(Seq(0)), -1);
    /// assert_eq!(Seq(2).dist_to(Seq(0)), -2);
    ///
    /// assert_eq!(Seq(0).dist_to(Seq(u16::MAX)), -1);
    /// assert_eq!(Seq(u16::MAX).dist_to(Seq(u16::MAX)), 0);
    ///
    /// assert_eq!(Seq(u16::MAX).dist_to(Seq(0)), 1);
    /// assert_eq!((Seq(u16::MAX) - Seq(3)).dist_to(Seq(0)), 4);
    ///
    /// assert_eq!(Seq(u16::MAX).dist_to(Seq(3)), 4);
    /// assert_eq!((Seq(u16::MAX) - Seq(3)).dist_to(Seq(3)), 7);
    /// ```
    #[must_use]
    pub const fn dist_to(self, rhs: Self) -> i16 {
        #[expect(clippy::cast_possible_wrap, reason = "we want wrap behavior")]
        (rhs.0.wrapping_sub(self.0) as i16)
    }
}

impl fmt::Debug for Seq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Seq").field(&self.0).finish()
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

        #[expect(clippy::cast_possible_wrap, reason = "we want wrap behavior")]
        (s1 as i16).wrapping_sub(s2 as i16).cmp(&0)
    }
}

impl PartialOrd for Seq {
    /// See [`Seq::cmp`].
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Seq {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0.wrapping_add(rhs.0))
    }
}

impl AddAssign for Seq {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Seq {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
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

//
// `PacketSeq`
//

impl PacketSeq {
    /// Creates a new sequence number from a raw number.
    ///
    /// If you already have a [`Seq`], just wrap it in this type.
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

impl fmt::Debug for PacketSeq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("PacketSeq").field(&self.0 .0).finish()
    }
}

impl FixedEncodeLen for PacketSeq {
    const ENCODE_LEN: usize = Seq::ENCODE_LEN;
}

impl Encode for PacketSeq {
    type Error = <Seq as Encode>::Error;

    fn encode(&self, dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        self.0.encode(dst)
    }
}

impl Decode for PacketSeq {
    type Error = <Seq as Decode>::Error;

    fn decode(src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Seq::decode(src).map(Self)
    }
}
//
// `MessageSeq`
//

impl MessageSeq {
    /// Creates a new sequence number from a raw number.
    ///
    /// If you already have a [`Seq`], just wrap it in this type.
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

impl fmt::Debug for MessageSeq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MessageSeq").field(&self.0 .0).finish()
    }
}

impl FixedEncodeLen for MessageSeq {
    const ENCODE_LEN: usize = Seq::ENCODE_LEN;
}

impl Encode for MessageSeq {
    type Error = <Seq as Encode>::Error;

    fn encode(&self, dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        self.0.encode(dst)
    }
}

impl Decode for MessageSeq {
    type Error = <Seq as Decode>::Error;

    fn decode(src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Seq::decode(src).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, octs::test::*};

    #[test]
    fn encode_decode_all_seqs() {
        for seq in 0..u16::MAX {
            hint_round_trip(&Seq(seq));
        }
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
