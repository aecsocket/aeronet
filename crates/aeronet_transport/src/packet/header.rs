use {
    super::{Acknowledge, PacketHeader, PacketSeq},
    octs::{
        BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLenHint, Read, VarInt, VarIntTooLarge,
        Write,
    },
    std::convert::Infallible,
};

impl FixedEncodeLenHint for PacketHeader {
    const MIN_ENCODE_LEN: usize =
        PacketSeq::MIN_ENCODE_LEN + Acknowledge::MIN_ENCODE_LEN + VarInt::<u16>::MIN_ENCODE_LEN;

    const MAX_ENCODE_LEN: usize =
        PacketSeq::MAX_ENCODE_LEN + Acknowledge::MAX_ENCODE_LEN + VarInt::<u16>::MAX_ENCODE_LEN;
}

impl EncodeLen for PacketHeader {
    fn encode_len(&self) -> usize {
        self.seq.encode_len() + self.acks.encode_len() + VarInt(self.ack_delay).encode_len()
    }
}

impl Encode for PacketHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.seq)?;
        dst.write(&self.acks)?;
        dst.write(&VarInt(self.ack_delay))?;
        Ok(())
    }
}

impl Decode for PacketHeader {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            seq: src.read()?,
            acks: src.read()?,
            ack_delay: src.read::<VarInt<_>>()?.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use octs::test::*;

    #[test]
    fn encode_decode() {
        hint_round_trip(&PacketHeader {
            seq: PacketSeq::new(3),
            acks: Acknowledge {
                last_recv: PacketSeq::new(2),
                bits: 0b11,
            },
            ack_delay: 123,
        });
    }
}
