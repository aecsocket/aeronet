//! Handles splitting and reassembling a single large message into multiple
//! smaller packets for sending over a network.
//!
//! # Memory management
//!
//! The initial implementation used a fixed-size "sequence buffer" data
//! structure as proposed by [*Gaffer On Games*], however this is an issue when
//! we don't know how many fragments and messages we may be receiving, as this
//! buffer is able to run out of space. This current implementation, instead,
//! uses a map to store messages. This is able to grow infinitely, or at least
//! up to how much memory the computer has.
//!
//! Due to the fact that fragments may be dropped in transport, and that old
//! messages waiting for more fragments to be received may never get those
//! fragments, users should be careful to clean up fragments periodically -
//! see [`FragmentReceiver::clean_up`].
//!
//! [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#data-structure-on-receiver-side

use aeronet::{
    integer_encoding::VarInt,
    octs::{self, ConstEncodeLen},
};
use arbitrary::Arbitrary;

use crate::seq::Seq;

mod fragmentation;
mod reassembly;

pub use {fragmentation::*, reassembly::*};

/// Metadata for a packet produced by [`Fragmentation::fragment`] and read by
/// [`Fragmentation::reassemble`].
#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct FragmentHeader {
    /// Sequence number of the message that this fragment is a part of.
    pub msg_seq: Seq,
    /// How many fragments this packet's message is split up into.
    pub num_frags: u8,
    /// Index of this fragment in the total message.
    pub frag_id: u8,
}

impl octs::ConstEncodeLen for FragmentHeader {
    const ENCODE_LEN: usize = Seq::ENCODE_LEN + u8::ENCODE_LEN + u8::ENCODE_LEN;
}

impl octs::Encode for FragmentHeader {
    fn encode(&self, buf: &mut impl octs::WriteBytes) -> octs::Result<()> {
        buf.write(&self.msg_seq)?;
        buf.write(&self.num_frags)?;
        buf.write(&self.frag_id)?;
        Ok(())
    }
}

impl octs::Decode for FragmentHeader {
    fn decode(buf: &mut impl octs::ReadBytes) -> octs::Result<Self> {
        Ok(Self {
            msg_seq: buf.read()?,
            num_frags: buf.read()?,
            frag_id: buf.read()?,
        })
    }
}

/// Fragment of a message as it is encoded inside a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment<B> {
    /// Metadata of this fragment, such as which message this fragment is a part
    /// of.
    pub header: FragmentHeader,
    /// Buffer storing the message payload of this fragment.
    pub payload: B,
}

impl<B: bytes::Buf> Fragment<B> {
    /// Writes this value into a [`WriteBytes`].
    ///
    /// This is equivalent to [`Encode`], but consumes `self` instead of taking
    /// a shared reference. This is because we consume the payload when writing
    /// it into a buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    ///
    /// [`Encode`]: octs::Encode
    pub fn encode_into(mut self, buf: &mut impl octs::WriteBytes) -> octs::Result<()> {
        buf.write(&self.header)?;
        // if B is Bytes, this will be nearly free -
        // doesn't even increment the ref count
        let payload = self.payload.copy_to_bytes(self.payload.remaining());
        buf.write(&payload)?;
        Ok(())
    }
}

impl<B: bytes::Buf> octs::EncodeLen for Fragment<B> {
    fn encode_len(&self) -> usize {
        let len = self.payload.remaining();
        FragmentHeader::ENCODE_LEN + VarInt::required_space(len) + len
    }
}

impl octs::Decode for Fragment<bytes::Bytes> {
    fn decode(buf: &mut impl octs::ReadBytes) -> octs::Result<Self> {
        Ok(Self {
            header: buf.read()?,
            payload: buf.read()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use bytes::{Bytes, BytesMut};

    use aeronet::octs::{ConstEncodeLen, ReadBytes, WriteBytes};

    use super::*;

    #[test]
    fn encode_decode_header() {
        let v = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 12,
            frag_id: 34,
        };
        let mut buf = BytesMut::with_capacity(FragmentHeader::ENCODE_LEN);

        buf.write(&v).unwrap();
        assert_eq!(FragmentHeader::ENCODE_LEN, buf.len());

        assert_eq!(v, buf.freeze().read::<FragmentHeader>().unwrap());
    }

    const PAYLOAD_LEN: usize = 1024;

    const MSG1: Bytes = Bytes::from_static(b"Message 1");
    const MSG2: Bytes = Bytes::from_static(b"Message 2");
    const MSG3: Bytes = Bytes::from_static(b"Message 3");

    fn frag() -> (Fragmentation, Reassembly) {
        (
            Fragmentation::new(PAYLOAD_LEN),
            Reassembly::new(PAYLOAD_LEN),
        )
    }

    #[test]
    fn single_in_order() {
        let (frag, mut asm) = frag();
        let p1 = frag.fragment(Seq(0), MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment(Seq(1), MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment(Seq(2), MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG1,
            asm.reassemble(&p1.header, &p1.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG2,
            asm.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG3,
            asm.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn single_out_of_order() {
        let (frag, mut asm) = frag();
        let p1 = frag.fragment(Seq(0), MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment(Seq(1), MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment(Seq(2), MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG3,
            asm.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG1,
            asm.reassemble(&p1.header, &p1.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG2,
            asm.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn large1() {
        let (frag, mut asm) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN + 1));
        let [p1, p2] = frag
            .fragment(Seq(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(asm.reassemble(&p1.header, &p1.payload), Ok(None));
        assert_eq!(
            msg,
            asm.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn large2() {
        let (frag, mut asm) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN * 2 + 1));
        let [p1, p2, p3] = frag
            .fragment(Seq(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(asm.reassemble(&p1.header, &p1.payload), Ok(None));
        assert_matches!(asm.reassemble(&p2.header, &p2.payload), Ok(None));
        assert_eq!(
            msg,
            asm.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
    }
}
