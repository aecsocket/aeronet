//! See [`PacketHeader`].

use std::convert::Infallible;

use octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write};

use crate::ty::{Acknowledge, PacketHeader, PacketSeq};

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
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            seq: src.read()?,
            acks: src.read()?,
        })
    }
}
