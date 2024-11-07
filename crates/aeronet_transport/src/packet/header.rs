use {
    super::{Acknowledge, PacketHeader, PacketSeq},
    core::convert::Infallible,
    octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, VarIntTooLarge, Write},
};

impl FixedEncodeLen for PacketHeader {
    const ENCODE_LEN: usize = PacketSeq::ENCODE_LEN + Acknowledge::ENCODE_LEN;
}

impl Encode for PacketHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.seq)?;
        dst.write(&self.acks)?;
        Ok(())
    }
}

impl Decode for PacketHeader {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            seq: src.read()?,
            acks: src.read()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use {super::*, octs::test::*};

    #[test]
    fn encode_decode() {
        hint_round_trip(&PacketHeader {
            seq: PacketSeq::new(3),
            acks: Acknowledge {
                last_recv: PacketSeq::new(2),
                bits: 0b11,
            },
        });
    }
}
