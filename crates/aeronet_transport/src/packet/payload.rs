use {
    super::{FragmentPayload, FragmentPayloadLen},
    octs::{
        BufError, BufTooShortOr, Decode, Encode, EncodeLen, Read, VarInt, VarIntTooLarge, Write,
    },
    thiserror::Error,
};

/// Attempted to [`Encode`] a [`FragmentPayload`] which was more than
/// [`FragmentPayloadLen::MAX`] bytes long.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Error)]
#[error("payload too large - {len} / {} bytes", FragmentPayloadLen::MAX)]
pub struct PayloadTooLarge {
    /// Length of the [`FragmentPayload`].
    pub len: usize,
}

impl BufError for PayloadTooLarge {}

impl EncodeLen for FragmentPayload {
    fn encode_len(&self) -> usize {
        let len_u = self.0.len();
        let Ok(len) = FragmentPayloadLen::try_from(len_u) else {
            return 0;
        };

        VarInt(len).encode_len() + len_u
    }
}

impl Encode for FragmentPayload {
    type Error = PayloadTooLarge;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        let len_u = self.0.len();
        let len =
            FragmentPayloadLen::try_from(len_u).map_err(|_| PayloadTooLarge { len: len_u })?;

        dst.write(VarInt(len))?;
        dst.write_from(self.0.clone())?;
        Ok(())
    }
}

impl Decode for FragmentPayload {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        let len = src.read::<VarInt<FragmentPayloadLen>>()?.0;
        let len_u = usize::try_from(len)
            .expect("`FragmentPayloadLen` is checked to be at least the size of `usize`");
        Ok(Self(src.read_next(len_u)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use octs::{test::*, Bytes};

    #[test]
    fn encode_decode() {
        round_trip(&FragmentPayload(Bytes::from_static(&[])));
        round_trip(&FragmentPayload(Bytes::from_static(&[1, 2, 3, 4])));
    }
}
