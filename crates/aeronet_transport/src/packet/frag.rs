use {
    super::{
        Fragment, FragmentHeader, FragmentIndex, FragmentPosition, MessageSeq, PayloadTooLarge,
    },
    crate::lane::LaneIndex,
    core::{convert::Infallible, fmt},
    octs::{
        BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLenHint, Read, VarInt, VarIntTooLarge,
        Write,
    },
};

// `FragmentPosition`

impl FragmentPosition {
    /// Creates a position for a fragment which is *not* the last one in the
    /// message.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::packet::{FragmentIndex, FragmentPosition};
    ///
    /// let pos = FragmentPosition::non_last(3).unwrap();
    /// assert_eq!(3, pos.index());
    /// assert!(!pos.is_last());
    /// ```
    #[must_use]
    pub fn non_last(index: FragmentIndex) -> Option<Self> {
        index.checked_mul(2).map(Self)
    }

    /// Creates a position for a fragment which *is* the last one in the
    /// message.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::packet::{FragmentIndex, FragmentPosition};
    ///
    /// let pos = FragmentPosition::last(3).unwrap();
    /// assert_eq!(3, pos.index());
    /// assert!(pos.is_last());
    /// ```
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

impl FixedEncodeLenHint for FragmentPosition {
    const MIN_ENCODE_LEN: usize = <VarInt<FragmentIndex> as FixedEncodeLenHint>::MIN_ENCODE_LEN;

    const MAX_ENCODE_LEN: usize = <VarInt<FragmentIndex> as FixedEncodeLenHint>::MAX_ENCODE_LEN;
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

// `FragmentHeader`

impl FixedEncodeLenHint for FragmentHeader {
    const MIN_ENCODE_LEN: usize =
        LaneIndex::MIN_ENCODE_LEN + MessageSeq::MIN_ENCODE_LEN + FragmentPosition::MIN_ENCODE_LEN;

    const MAX_ENCODE_LEN: usize =
        LaneIndex::MAX_ENCODE_LEN + MessageSeq::MAX_ENCODE_LEN + FragmentPosition::MAX_ENCODE_LEN;
}

impl EncodeLen for FragmentHeader {
    fn encode_len(&self) -> usize {
        self.lane.encode_len() + self.seq.encode_len() + self.position.encode_len()
    }
}

impl Encode for FragmentHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.seq)?;
        dst.write(self.lane)?;
        dst.write(self.position)?;
        Ok(())
    }
}

impl Decode for FragmentHeader {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            seq: src.read()?,
            lane: src.read()?,
            position: src.read()?,
        })
    }
}

// `Fragment`

impl EncodeLen for Fragment {
    fn encode_len(&self) -> usize {
        self.header.encode_len() + self.payload.encode_len()
    }
}

impl Encode for Fragment {
    type Error = PayloadTooLarge;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.header)?;
        dst.write(&self.payload)?;
        Ok(())
    }
}

impl Decode for Fragment {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            header: src.read()?,
            payload: src.read()?,
        })
    }
}
