use {
    super::FragmentPayload,
    crate::size::MinSize,
    derive_more::{Display, Error},
    octs::{BufError, BufTooShortOr, Decode, Encode, EncodeLen, Read, VarIntTooLarge, Write},
};

/// Attempted to [`Encode`] a [`FragmentPayload`] which was more than
/// [`MinSize::MAX`] bytes long.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Display, Error)]
#[display("payload too large - {len} / {} bytes", MinSize::MAX.0)]
pub struct PayloadTooLarge {
    /// Length of the [`FragmentPayload`].
    pub len: usize,
}

impl BufError for PayloadTooLarge {}

impl EncodeLen for FragmentPayload {
    fn encode_len(&self) -> usize {
        let len_u = self.0.len();
        let Ok(len) = MinSize::try_from(len_u) else {
            return 0;
        };
        len.encode_len() + len_u
    }
}

impl Encode for FragmentPayload {
    type Error = PayloadTooLarge;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        let len = self.0.len();
        let len = MinSize::try_from(len).map_err(|_| PayloadTooLarge { len })?;

        dst.write(len)?;
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
