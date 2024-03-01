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
}
