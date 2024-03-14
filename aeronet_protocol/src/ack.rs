use arbitrary::Arbitrary;

use crate::{
    bytes::{BytesError, ReadBytes, WriteBytes},
    seq::Seq,
};

#[derive(Debug)]
pub struct Acknowledge {
    last_recv: Seq,
    ack_bits: u32,
}

impl Acknowledge {
    pub fn new() -> Self {
        Self {
            last_recv: Seq(0),
            ack_bits: 0,
        }
    }

    pub fn ack(&mut self, seq: Seq) {
        let delta = (seq - self.last_recv).0;
        if delta > 0 {
            // `seq` is the new last recv seq
            self.last_recv = seq;
            //    seq: 8
            //    last_recv: 3
            // -> delta: 5
            //    old recv_bits: 0b00..000000001000
            //                                 ^
            //                                 |  shifted `delta` (5) places
            //                            v----+
            //    new recv_bits: 0b00..000100000000
            self.ack_bits <<= delta;
        } else {
            // `seq` marks an ack some time before `last_recv`,
            // so we set a bit in the recv bitfield
            // `delta = 0` falls under this case as well;
            // we confirm that we've received the last received seq
            self.ack_bits |= 1 << delta;
        }
    }

    pub fn header(&self) -> AckHeader {
        AckHeader {
            last_recv: self.last_recv,
            ack_bits: self.ack_bits,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct AckHeader {
    pub last_recv: Seq,
    pub ack_bits: u32,
}

impl AckHeader {
    /// [Encoded] size of this value in bytes.
    ///
    /// [Encoded]: AckHeader::encode
    pub const ENCODE_SIZE: usize = Seq::ENCODE_SIZE + std::mem::size_of::<u32>();

    /// Converts this into an iterator over all [`Seq`]s this header contains.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_protocol::{seq::Seq, ack::AckHeader};
    /// let header = AckHeader {
    ///     last_recv: Seq(50),
    ///     ack_bits: 0b0010010,
    /// };
    /// let mut iter = header.seqs();
    /// assert_eq!(Seq(49), iter.next().unwrap());
    /// assert_eq!(Seq(46), iter.next().unwrap());
    /// assert_eq!(None, iter.next());
    /// ```
    pub fn seqs(self) -> impl Iterator<Item = Seq> {
        // explicitly don't ack `last_recv` *unless* bit 0 is set
        // we may be in a situation where we literally haven't received any of
        // the last 32 packets, so it'd be invalid to ack the `last_recv`
        (0..32).filter_map(move |bit_index| {
            let packet_seq = Seq(self.last_recv.0.wrapping_sub(bit_index));
            if self.ack_bits & (1 << bit_index) == 0 {
                None
            } else {
                Some(packet_seq)
            }
        })
    }

    pub fn encode(&self, buf: &mut impl WriteBytes) -> Result<(), BytesError> {
        self.last_recv.encode(buf)?;
        buf.write_u32(self.ack_bits)?;
        Ok(())
    }

    pub fn decode(buf: &mut impl ReadBytes) -> Result<Self, BytesError> {
        Ok(Self {
            last_recv: Seq::decode(buf)?,
            ack_bits: buf.read_u32()?,
        })
    }
}
