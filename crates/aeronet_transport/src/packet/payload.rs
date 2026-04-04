use {
    super::FragmentPayload,
    crate::size::MinSize,
    core::convert::Infallible,
    octs::{BufTooShortOr, Bytes, Decode, Encode, EncodeLen, Read, VarIntTooLarge, Write},
};

impl FragmentPayload {
    /// Creates a new empty [`FragmentPayload`] with no bytes.
    #[must_use]
    pub const fn empty() -> Self {
        Self(Bytes::new())
    }

    /// Creates a new [`FragmentPayload`], validating that its length fits
    /// within [`MinSize`].
    ///
    /// If `bytes.len()` does not fit in a [`MinSize`], returns [`None`].
    pub fn new(bytes: Bytes) -> Option<Self> {
        let len = bytes.len();
        MinSize::try_from(len).map(|_| Self(bytes)).ok()
    }

    /// Returns the number of bytes contained in this `Bytes`.
    ///
    /// This is checked at construction time to fit into a [`MinSize`].
    #[must_use]
    pub const fn len(&self) -> MinSize {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "we check at construction time that `self.0.len()` fits into a `MinSize`"
        )]
        MinSize(self.0.len() as u32)
    }

    /// Returns the number of bytes contained in this `Bytes`, as a [`usize`].
    ///
    /// This is checked at construction time to fit into a [`MinSize`].
    #[must_use]
    pub const fn len_usize(&self) -> usize {
        self.len().0 as usize
    }
}

impl EncodeLen for FragmentPayload {
    fn encode_len(&self) -> usize {
        let len = self.len();
        len.encode_len() + usize::from(len)
    }
}

impl Encode for FragmentPayload {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.len())?;
        dst.write_from(self.0.clone())?;
        Ok(())
    }
}

impl Decode for FragmentPayload {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        let len = src.read::<MinSize>()?.0 as usize;
        Ok(Self(src.read_next(len)?))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        octs::{Bytes, test::*},
    };

    #[test]
    fn encode_decode() {
        round_trip(&FragmentPayload(Bytes::from_static(&[])));
        round_trip(&FragmentPayload(Bytes::from_static(&[1, 2, 3, 4])));
    }
}
