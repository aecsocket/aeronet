use {
    super::MessagePayload,
    octs::{
        BufError, BufTooShortOr, Decode, Encode, EncodeLen, Read, VarInt, VarIntTooLarge, Write,
    },
    std::mem::size_of,
    thiserror::Error,
};

type PayloadLen = u32;

const _: () = {
    assert!(size_of::<usize>() >= size_of::<PayloadLen>());
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Error)]
#[error("payload too large - {len} / {} bytes", PayloadLen::MAX)]
pub struct PayloadTooLarge {
    pub len: usize,
}

impl BufError for PayloadTooLarge {}

impl EncodeLen for MessagePayload {
    fn encode_len(&self) -> usize {
        let len_u = self.0.len();
        let Ok(len) = PayloadLen::try_from(len_u) else {
            return 0;
        };

        VarInt(len).encode_len() + len_u
    }
}

impl Encode for MessagePayload {
    type Error = PayloadTooLarge;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        let len_u = self.0.len();
        let len = PayloadLen::try_from(len_u).map_err(|_| PayloadTooLarge { len: len_u })?;

        dst.write(VarInt(len))?;
        dst.write_from(self.0.clone())?;
        Ok(())
    }
}

impl Decode for MessagePayload {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        let len = src.read::<VarInt<PayloadLen>>()?.0;
        let len_u = usize::try_from(len).expect("bit sizes checked at compile time");
        Ok(Self(src.read_next(len_u)?))
    }
}
