//! See [`PacketHeader`].

use std::convert::Infallible;

use octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write};

use crate::ty::{Acknowledge, PacketFlags, PacketHeader, PacketSeq};

const ACK_BITS_MASK: u32 = 0b01111111_11111111_11111111_11111111;

impl FixedEncodeLen for PacketHeader {
    const ENCODE_LEN: usize = PacketSeq::ENCODE_LEN + PacketSeq::ENCODE_LEN + u32::ENCODE_LEN;
}

impl Encode for PacketHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.seq)?;
        dst.write(&self.acks.last_recv)?;

        let mut bits = 0u32;
        bits |= self.acks.bits & ACK_BITS_MASK;
        bits |= (self.flags.bits() as u32) << 31;
        dst.write(&bits)?;
        Ok(())
    }
}

impl Decode for PacketHeader {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        let seq = src.read()?;
        let last_recv = src.read()?;
        let bits = src.read::<u32>()?;
        let ack_bits = bits & ACK_BITS_MASK;
        let flags = PacketFlags::from_bits_retain((bits >> 31) as u8);
        Ok(Self {
            seq,
            acks: Acknowledge {
                last_recv,
                bits: ack_bits,
            },
            flags,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::u32;

    use octs::test::*;

    use crate::ty::Seq;

    use super::*;

    #[test]
    fn encode_decode() {
        hint_round_trip(&PacketHeader {
            seq: PacketSeq::new(0),
            acks: Acknowledge {
                last_recv: PacketSeq::new(0),
                bits: 0,
            },
            flags: PacketFlags::empty(),
        });
        hint_round_trip(&PacketHeader {
            seq: PacketSeq(Seq::MAX),
            acks: Acknowledge {
                last_recv: PacketSeq(Seq::MAX),
                bits: u32::MAX & ACK_BITS_MASK,
            },
            flags: PacketFlags::all(),
        });
    }
}
