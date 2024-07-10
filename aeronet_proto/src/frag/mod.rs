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
//! fragments, you need a strategy to handle fragments which may never be fully
//! reassembled. Some possible strategies are:
//! * for unreliable lanes
//!   * incomplete messages are removed if they have not received a new fragment
//!     in X milliseconds
//!   * if a new message comes in and it takes more memory than is available,
//!     the oldest existing messages are removed until there is enough memory
//! * for reliable lanes
//!   * if we don't have enough memory to fit a new message in, the connection
//!     is reset
//!
//! This is automatically handled in [`session`](crate::session).
//!
//! [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#data-structure-on-receiver-side

use std::convert::Infallible;

use arbitrary::Arbitrary;
use octs::{
    BufTooShortOr, Bytes, Decode, Encode, FixedEncodeLen, Read, VarInt, VarIntTooLarge, Write,
};

use crate::seq::Seq;

mod recv;
mod send;

pub use {recv::*, send::*};

/// Metadata for a packet produced by [`FragmentSender::fragment`] and read by
/// [`FragmentReceiver::reassemble`].
#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct FragmentHeader {
    /// Sequence number of the message that this fragment is a part of.
    pub msg_seq: Seq,
    /// How many fragments this packet's message is split up into.
    pub num_frags: u8,
    /// Index of this fragment in the complete message.
    pub frag_index: u8,
}

impl FixedEncodeLen for FragmentHeader {
    const ENCODE_LEN: usize = Seq::ENCODE_LEN + u8::ENCODE_LEN + u8::ENCODE_LEN;
}

impl Encode for FragmentHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.msg_seq)?;
        dst.write(&self.num_frags)?;
        dst.write(&self.frag_index)?;
        Ok(())
    }
}

impl Decode for FragmentHeader {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            msg_seq: src.read()?,
            num_frags: src.read()?,
            frag_index: src.read()?,
        })
    }
}

/// Fragment of a message as it is encoded inside a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// Metadata of this fragment, such as which message this fragment is a part
    /// of.
    pub header: FragmentHeader,
    /// Buffer storing the message payload of this fragment.
    pub payload: Bytes,
}

impl Encode for Fragment {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.header)?;
        dst.write(VarInt(self.payload.len()))?;
        dst.write_from(self.payload.clone())?;
        Ok(())
    }
}

impl Decode for Fragment {
    type Error = VarIntTooLarge;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        let header = src.read()?;
        let payload_len = src.read::<VarInt<usize>>()?.0;
        let payload = src.read_next(payload_len)?;
        Ok(Self { header, payload })
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use octs::{Bytes, BytesMut};

    use super::*;

    #[test]
    fn encode_decode_header() {
        let v = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 12,
            frag_index: 34,
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

    fn frag() -> (FragmentSender, FragmentReceiver) {
        (
            FragmentSender::new(PAYLOAD_LEN),
            FragmentReceiver::new(PAYLOAD_LEN),
        )
    }

    #[test]
    fn single_in_order() {
        let (send, mut recv) = frag();
        let p1 = send.fragment(Seq(0), MSG1).unwrap().next().unwrap();
        let p2 = send.fragment(Seq(1), MSG2).unwrap().next().unwrap();
        let p3 = send.fragment(Seq(2), MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG1,
            recv.reassemble(&p1.header, &p1.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG2,
            recv.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG3,
            recv.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn single_out_of_order() {
        let (send, mut recv) = frag();
        let p1 = send.fragment(Seq(0), MSG1).unwrap().next().unwrap();
        let p2 = send.fragment(Seq(1), MSG2).unwrap().next().unwrap();
        let p3 = send.fragment(Seq(2), MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG3,
            recv.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG1,
            recv.reassemble(&p1.header, &p1.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG2,
            recv.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn large1() {
        let (send, mut recv) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN + 1));
        let [p1, p2] = send
            .fragment(Seq(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(recv.reassemble(&p1.header, &p1.payload), Ok(None));
        assert_eq!(
            msg,
            recv.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn large2() {
        let (send, mut recv) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN * 2 + 1));
        let [p1, p2, p3] = send
            .fragment(Seq(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(recv.reassemble(&p1.header, &p1.payload), Ok(None));
        assert_matches!(recv.reassemble(&p2.header, &p2.payload), Ok(None));
        assert_eq!(
            msg,
            recv.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
    }
}
