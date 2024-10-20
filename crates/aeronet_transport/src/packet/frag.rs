use {
    super::{FragmentPosition, MessageFragment, MessageSeq, PayloadTooLarge},
    core::fmt,
    octs::{
        BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLen, Read, VarInt, VarIntTooLarge,
        Write,
    },
};

//
// `MessageFragment`
//

impl EncodeLen for MessageFragment {
    fn encode_len(&self) -> usize {
        MessageSeq::ENCODE_LEN
            + self.lane.encode_len()
            + self.pos.encode_len()
            + self.payload.encode_len()
    }
}

impl Encode for MessageFragment {
    type Error = PayloadTooLarge;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.seq)?;
        dst.write(self.lane)?;
        dst.write(self.pos)?;
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
            pos: src.read()?,
            payload: src.read()?,
        })
    }
}

//
// `FragmentPosition`
//

impl FragmentPosition {
    #[must_use]
    pub const fn non_last_u32(index: u32) -> Self {
        Self(index as u64 * 2)
    }

    #[must_use]
    pub const fn last_u32(index: u32) -> Self {
        Self(index as u64 * 2 + 1)
    }

    #[must_use]
    pub fn non_last_u64(index: u64) -> Option<Self> {
        index.checked_mul(2).map(Self)
    }

    #[must_use]
    pub fn last_u64(index: u64) -> Option<Self> {
        index
            .checked_mul(2)
            .and_then(|n| n.checked_add(1))
            .map(Self)
    }

    #[must_use]
    pub fn index(self) -> u64 {
        self.0 / 2
    }

    #[must_use]
    pub const fn is_last(self) -> bool {
        self.0 % 2 == 0
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
    type Error = <VarInt<u64> as Encode>::Error;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(VarInt(self.0))
    }
}

impl Decode for FragmentPosition {
    type Error = <VarInt<u64> as Decode>::Error;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self(src.read::<VarInt<u64>>()?.0))
    }
}
