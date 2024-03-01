use arbitrary::Arbitrary;
use bytes::{BufMut, Bytes, BytesMut};
use safer_bytes::SafeBuf;

use crate::{bytes::ReadError, seq::Seq};

#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct AckHeader {
    pub packet_seq: Seq,
    pub last_packet_ack: Seq,
    pub ack_bits: u32,
}

impl AckHeader {
    /// [Encoded] size of this value in bytes.
    ///
    /// [Encoded]: AckHeader::encode
    pub const ENCODE_SIZE: usize = Seq::ENCODE_SIZE + Seq::ENCODE_SIZE + std::mem::size_of::<u32>();

    /// Encodes this value into a byte buffer.
    ///
    /// The buffer should have at least [`ENCODE_SIZE`] bytes of capacity, to
    /// not have to allocate more space.
    ///
    /// [`ENCODE_SIZE`]: AckHeader::ENCODE_SIZE
    pub fn encode(&self, buf: &mut BytesMut) {
        self.packet_seq.encode(buf);
        self.last_packet_ack.encode(buf);
        buf.put_u32(self.ack_bits);
    }

    /// Decodes this value from a byte buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is shorter than [`ENCODE_SIZE`].
    ///
    /// [`ENCODE_SIZE`]: AckHeader::ENCODE_SIZE
    pub fn decode(buf: &mut Bytes) -> Result<Self, ReadError> {
        let packet_seq = Seq::decode(buf)?;
        let last_packet_ack = Seq::decode(buf)?;
        let ack_bits = buf.try_get_u32()?;
        Ok(Self {
            packet_seq,
            last_packet_ack,
            ack_bits,
        })
    }

    /// Gets an iterator over all [`Seq`]s which this header encodes.
    pub fn acks(&self) -> impl Iterator<Item = Seq> + '_ {
        // explicitly don't ack `acks.packet_seq` *unless* bit 0 is set
        // we may be in a situation where we literally haven't received any of
        // the last 32 packets, so it'd be invalid to ack the `packet_seq`
        let mut bit_index = 0;
        std::iter::from_fn(move || {
            while bit_index < 32 {
                let packet_seq = Seq(self.packet_seq.0.wrapping_sub(bit_index));
                let is_set = self.ack_bits & (1 << bit_index) != 0;
                bit_index += 1;
                if is_set {
                    return Some(packet_seq);
                }
            }
            None
        })
    }
}

#[derive(Debug)]
pub struct AckReceiver {
    last_recv: Seq,
    recv_bits: u32,
}

impl AckReceiver {
    pub fn ack(&mut self, seq: Seq) {
        let delta = (seq - self.last_recv).0;
        if delta > 0 {
            // `seq` is the new last recv seq
            self.last_recv = seq;
            //    packet_seq: 8
            //    last_recv: 3
            // -> delta: 5
            //    old recv_bits: 0b00..000000001000
            //                                 ^
            //                                 |  shifted `delta` (5) places
            //                            v----+
            //    new recv_bits: 0b00..000100000000
            self.recv_bits << delta;
        } else {
            // `seq` marks an ack some time before `last_recv`,
            // so we set a bit in the recv bitfield
            // `delta = 0` falls under this case as well;
            // we confirm that we've received the last received seq
            self.recv_bits |= 1 << delta;
        }
    }
}
