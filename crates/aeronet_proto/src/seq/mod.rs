//! See [`Seq`].

mod buf;

pub use buf::*;
use {
    crate::ty::{MessageSeq, PacketSeq, Seq},
    octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write},
    std::{
        cmp::Ordering,
        convert::Infallible,
        fmt,
        ops::{Add, AddAssign, Sub, SubAssign},
    },
};

impl Seq {
    /// Sequence number with value `0`.
    pub const ZERO: Self = Self(0);

    /// Sequence number with value [`u16::MAX`].
    pub const MAX: Self = Self(u16::MAX);

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
    /// # Examples
    ///
    /// ```
    /// # use aeronet_proto::ty::Seq;
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

impl PacketSeq {
    /// Sequence number `0`.
    pub const ZERO: Self = Self::new(0);

    /// Sequence number `1`.
    pub const ONE: Self = Self::new(1);

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
        let Self(Seq(seq)) = self;
        write!(f, "PacketSeq({seq})")
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

impl MessageSeq {
    /// Sequence number `0`.
    pub const ZERO: Self = Self::new(0);

    /// Sequence number `1`.
    pub const ONE: Self = Self::new(1);

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
        let Self(Seq(seq)) = self;
        write!(f, "MessageSeq({seq})")
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
