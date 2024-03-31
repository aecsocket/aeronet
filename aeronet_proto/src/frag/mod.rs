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
//! see [`Fragmentation::clean_up`].
//!
//! [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#data-structure-on-receiver-side

use std::{num::NonZeroU8, time::Instant};

use aeronet::octs;
use ahash::AHashMap;
use arbitrary::Arbitrary;
use bitvec::array::BitArray;

use crate::seq::Seq;

mod fragmentation;
mod reassembly;

pub use {fragmentation::*, reassembly::*};

/// Handles splitting a single large message into multiple smaller packets for
/// sending over a network, and rebuilding them back into a message on the other
/// side.
///
/// See the [module-level documentation](crate::frag).
#[derive(Debug)]
pub struct Fragmentation {
    payload_len: usize,
    messages: AHashMap<Seq, MessageBuffer>,
}

impl Fragmentation {
    /// Creates a new fragment sender/receiver from the given configuration.
    ///
    /// * `payload_len` defines the maximum length, in bytes, that the payload
    ///   of a single fragmented packet can be. This must be greater than 0.
    ///
    /// # Panics
    ///
    /// Panics if `payload_len` is 0.
    #[must_use]
    pub fn new(payload_len: usize) -> Self {
        assert!(payload_len > 0);
        Self {
            payload_len,
            messages: AHashMap::new(),
        }
    }

    /// Gets the maximum length of the payload of a fragment produced by
    /// [`Fragmentation::fragment`], and the expected length of the payload of
    /// a fragment received by [`Fragmentation::reassemble`].
    #[must_use]
    pub fn payload_len(&self) -> usize {
        self.payload_len
    }
}

#[derive(Debug, Clone)]
struct MessageBuffer {
    num_frags: NonZeroU8,
    num_frags_recv: u8,
    recv_frags: BitArray<[u8; 32]>,
    payload: Vec<u8>,
    last_recv_at: Instant,
}

/// Metadata for a packet produced by [`Fragmentation::fragment`] and read by
/// [`Fragmentation::reassemble`].
#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct FragHeader {
    /// Sequence number of the message that this fragment is a part of.
    pub msg_seq: Seq,
    /// How many fragments this packet's message is split up into.
    pub num_frags: u8,
    /// Index of this fragment in the total message.
    pub frag_id: u8,
}

impl octs::ConstEncodeLen for FragHeader {
    const ENCODE_LEN: usize = Seq::ENCODE_LEN + u8::ENCODE_LEN + u8::ENCODE_LEN;
}

impl octs::Encode for FragHeader {
    fn encode(&self, buf: &mut impl octs::WriteBytes) -> octs::Result<()> {
        buf.write(&self.msg_seq)?;
        buf.write(&self.num_frags)?;
        buf.write(&self.frag_id)?;
        Ok(())
    }
}

impl octs::Decode for FragHeader {
    fn decode(buf: &mut impl octs::ReadBytes) -> octs::Result<Self> {
        Ok(Self {
            msg_seq: buf.read()?,
            num_frags: buf.read()?,
            frag_id: buf.read()?,
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
        let v = FragHeader {
            msg_seq: Seq(1),
            num_frags: 12,
            frag_id: 34,
        };
        let mut buf = BytesMut::with_capacity(FragHeader::ENCODE_LEN);

        buf.write(&v).unwrap();
        assert_eq!(FragHeader::ENCODE_LEN, buf.len());

        assert_eq!(v, buf.freeze().read::<FragHeader>().unwrap());
    }

    const PAYLOAD_LEN: usize = 1024;

    const MSG1: Bytes = Bytes::from_static(b"Message 1");
    const MSG2: Bytes = Bytes::from_static(b"Message 2");
    const MSG3: Bytes = Bytes::from_static(b"Message 3");

    fn frag() -> Fragmentation {
        Fragmentation::new(PAYLOAD_LEN)
    }

    #[test]
    fn single_in_order() {
        let mut frag = frag();
        let p1 = frag.fragment(Seq(0), MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment(Seq(1), MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment(Seq(2), MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG1,
            frag.reassemble(&p1.header, &p1.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG2,
            frag.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG3,
            frag.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn single_out_of_order() {
        let mut frag = frag();
        let p1 = frag.fragment(Seq(0), MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment(Seq(1), MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment(Seq(2), MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG3,
            frag.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG1,
            frag.reassemble(&p1.header, &p1.payload).unwrap().unwrap()
        );
        assert_eq!(
            MSG2,
            frag.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn large1() {
        let mut frag = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN + 1));
        let [p1, p2] = frag
            .fragment(Seq(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(frag.reassemble(&p1.header, &p1.payload), Ok(None));
        assert_eq!(
            msg,
            frag.reassemble(&p2.header, &p2.payload).unwrap().unwrap()
        );
    }

    #[test]
    fn large2() {
        let mut frag = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN * 2 + 1));
        let [p1, p2, p3] = frag
            .fragment(Seq(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(frag.reassemble(&p1.header, &p1.payload), Ok(None));
        assert_matches!(frag.reassemble(&p2.header, &p2.payload), Ok(None));
        assert_eq!(
            msg,
            frag.reassemble(&p3.header, &p3.payload).unwrap().unwrap()
        );
    }
}
