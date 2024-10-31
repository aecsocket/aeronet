use {
    super::{FragmentIndex, FragmentPosition, MessageFragment, MessageSeq, PayloadTooLarge},
    octs::{
        BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLen, Read, VarInt, VarIntTooLarge,
        Write,
    },
    std::fmt,
};

//
// `MessageFragment`
//

impl EncodeLen for MessageFragment {
    fn encode_len(&self) -> usize {
        MessageSeq::ENCODE_LEN
            + self.lane.encode_len()
            + self.position.encode_len()
            + self.payload.encode_len()
    }
}

impl Encode for MessageFragment {
    type Error = PayloadTooLarge;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.seq)?;
        dst.write(self.lane)?;
        dst.write(self.position)?;
        dst.write(&self.payload)?;
        Ok(())
    }
}

impl Decode for MessageFragment {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            seq: src.read()?,
            lane: src.read()?,
            position: src.read()?,
            payload: src.read()?,
        })
    }
}

//
// `FragmentPosition`
//

impl FragmentPosition {
    /// Creates a position for a fragment which is *not* the last one in the
    /// message.
    #[must_use]
    pub fn non_last(index: FragmentIndex) -> Option<Self> {
        index.checked_mul(2).map(Self)
    }

    /// Creates a position for a fragment which *is* the last one in the
    /// message.
    #[must_use]
    pub fn last(index: FragmentIndex) -> Option<Self> {
        index
            .checked_mul(2)
            .and_then(|n| n.checked_add(1))
            .map(Self)
    }

    /// Creates a position which may be last or not.
    ///
    /// Prefer [`FragmentPosition::non_last`] or [`FragmentPosition::last`] if
    /// you know statically if the position is last or not.
    #[must_use]
    pub fn new(index: FragmentIndex, last: bool) -> Option<Self> {
        if last {
            Self::last(index)
        } else {
            Self::non_last(index)
        }
    }

    /// Gets the fragment index of this position.
    #[must_use]
    pub const fn index(self) -> FragmentIndex {
        self.0 / 2
    }

    /// Gets if this position represents the last fragment in a message.
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.0 % 2 == 1
    }
}

impl fmt::Debug for FragmentPosition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FragmentPosition")
            .field("index", &self.index())
            .field("is_last", &self.is_last())
            .finish()
    }
}

impl EncodeLen for FragmentPosition {
    fn encode_len(&self) -> usize {
        VarInt(self.0).encode_len()
    }
}

impl Encode for FragmentPosition {
    type Error = <VarInt<FragmentIndex> as Encode>::Error;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(VarInt(self.0))
    }
}

impl Decode for FragmentPosition {
    type Error = <VarInt<FragmentIndex> as Decode>::Error;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self(src.read::<VarInt<FragmentIndex>>()?.0))
    }
}
