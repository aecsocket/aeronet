use std::convert::Infallible;

use octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write};

use crate::{ack::Acknowledge, seq::Seq};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct PacketSeq(pub Seq);

impl PacketSeq {
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct MessageSeq(pub Seq);

impl MessageSeq {
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(Seq(n))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, arbitrary::Arbitrary)]
pub struct PacketHeader {
    pub packet_seq: PacketSeq,
    pub acks: Acknowledge,
}

impl FixedEncodeLen for PacketHeader {
    const ENCODE_LEN: usize = PacketSeq::ENCODE_LEN + Acknowledge::ENCODE_LEN;
}

impl Encode for PacketHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.packet_seq)?;
        dst.write(self.acks)?;
        Ok(())
    }
}

impl Decode for PacketHeader {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            packet_seq: src.read()?,
            acks: src.read()?,
        })
    }
}
