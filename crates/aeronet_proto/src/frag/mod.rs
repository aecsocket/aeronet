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

use octs::{
    BufTooShortOr, Bytes, Decode, Encode, EncodeLen, FixedEncodeLen, Read, VarInt, VarIntTooLarge,
    Write,
};

use crate::packet::MessageSeq;

mod recv;
mod send;

pub use {recv::*, send::*};

/// Indicates what index a [`Fragment`] represents, and whether this fragment
/// is the last fragment in a message.
///
/// When transmitting fragments to a peer, we need some way to tell if we have
/// received all of the fragments for a specific message. [*Gaffer On Games*]
/// uses two [`u8`]s, a `fragment id` and `num fragments`, to represent this
/// data. However, we do something smarter and use the MSB to indicate if this
/// fragment is the last one in the message. This leaves us with 128 possible
/// fragments per message, which should still be enough for most reasonable
/// use cases, but saves 1 byte of overhead per fragment per packet.
///
/// If the MSB is set, this fragment is the last one in this message. The other
/// 7 bits encode the index of this fragment in the message.
///
/// [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#fragment-packet-structure
// TODO docs
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, arbitrary::Arbitrary)]
pub struct FragmentMarker(pub(crate) u8);

const LAST_MASK: u8 = 0b1000_0000;

/// Maximum number of fragments that a message can be split into using
/// [`FragmentSender`].
///
/// See [`FragmentMarker`] for an explanation of how this value is determined.
pub const MAX_FRAGS: u8 = u8::MAX & !LAST_MASK;

impl FragmentMarker {
    /// Creates a new marker from a raw integer.
    #[inline]
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    /// Gets the raw integer from this fragment marker.
    ///
    /// To get the fragment index, use [`FragmentMarker::index`].
    #[inline]
    #[must_use]
    pub const fn into_raw(self) -> u8 {
        self.0
    }

    /// Creates a new marker from an index indicating that this **is not** the
    /// last fragment in the message.
    ///
    /// Returns [`None`] if the index is too large to be encoded properly.
    #[inline]
    #[must_use]
    pub const fn non_last(index: u8) -> Option<Self> {
        if index & LAST_MASK == 0 {
            Some(Self(index))
        } else {
            None
        }
    }

    /// Creates a new marker from an index indicating that this **is** the last
    /// fragment in the message.
    ///
    /// Returns [`None`] if the index is too large to be encoded properly.
    #[inline]
    #[must_use]
    pub const fn last(index: u8) -> Option<Self> {
        if index & LAST_MASK == 0 {
            Some(Self(index | LAST_MASK))
        } else {
            None
        }
    }

    /// Creates a new marker.
    ///
    /// If you know whether the marker is last or non-last at compile-time,
    /// prefer [`FragmentMarker::non_last`] or [`FragmentMarker::last`].
    #[inline]
    #[must_use]
    pub const fn new(index: u8, is_last: bool) -> Option<Self> {
        if is_last {
            Self::last(index)
        } else {
            Self::non_last(index)
        }
    }

    /// Gets the fragment index of this marker.
    #[inline]
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0 & !LAST_MASK
    }

    /// Gets if this fragment is the last one in the message.
    #[inline]
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.0 & LAST_MASK != 0
    }
}

/// Metadata for a packet produced by [`FragmentSender::fragment`] and read by
/// [`FragmentReceiver::reassemble`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, arbitrary::Arbitrary)]
pub struct FragmentHeader {
    /// Sequence number of the message that this fragment is a part of.
    pub msg_seq: MessageSeq,
    /// Marker of this fragment, indicating the fragment's index, and whether it
    /// is the last fragment of this message or not.
    pub marker: FragmentMarker,
}

impl FixedEncodeLen for FragmentHeader {
    const ENCODE_LEN: usize = MessageSeq::ENCODE_LEN + FragmentMarker::ENCODE_LEN;
}

impl Encode for FragmentHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&self.msg_seq)?;
        dst.write(&self.marker)?;
        Ok(())
    }
}

impl Decode for FragmentHeader {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            msg_seq: src.read()?,
            marker: src.read()?,
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

impl EncodeLen for Fragment {
    fn encode_len(&self) -> usize {
        self.header.encode_len() + VarInt(self.payload.len()).encode_len() + self.payload.len()
    }
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
    use web_time::Instant;

    use super::*;

    #[test]
    fn round_trip_header() {
        let v = FragmentHeader {
            msg_seq: MessageSeq::new(12),
            marker: FragmentMarker::from_raw(34),
        };
        let mut buf = BytesMut::with_capacity(FragmentHeader::ENCODE_LEN);

        buf.write(&v).unwrap();
        assert_eq!(FragmentHeader::ENCODE_LEN, buf.len());

        assert_eq!(v, buf.freeze().read::<FragmentHeader>().unwrap());
    }

    const PAYLOAD_LEN: usize = 64;

    const MSG1: Bytes = Bytes::from_static(b"Message 1");
    const MSG2: Bytes = Bytes::from_static(b"Message 2");
    const MSG3: Bytes = Bytes::from_static(b"Message 3");

    fn frag() -> (FragmentSender, FragmentReceiver) {
        (
            FragmentSender::new(PAYLOAD_LEN),
            FragmentReceiver::new(PAYLOAD_LEN),
        )
    }

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn single_in_order() {
        let (send, mut recv) = frag();
        let f1 = send
            .fragment(MessageSeq::new(0), MSG1)
            .unwrap()
            .next()
            .unwrap();
        let f2 = send
            .fragment(MessageSeq::new(1), MSG2)
            .unwrap()
            .next()
            .unwrap();
        let f3 = send
            .fragment(MessageSeq::new(2), MSG3)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(MSG1, recv.reassemble_frag(now(), f1).unwrap().unwrap());
        assert_eq!(MSG2, recv.reassemble_frag(now(), f2).unwrap().unwrap());
        assert_eq!(MSG3, recv.reassemble_frag(now(), f3).unwrap().unwrap());
    }

    #[test]
    fn single_out_of_order() {
        let (send, mut recv) = frag();
        let f1 = send
            .fragment(MessageSeq::new(0), MSG1)
            .unwrap()
            .next()
            .unwrap();
        let f2 = send
            .fragment(MessageSeq::new(1), MSG2)
            .unwrap()
            .next()
            .unwrap();
        let f3 = send
            .fragment(MessageSeq::new(2), MSG3)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(MSG3, recv.reassemble_frag(now(), f3).unwrap().unwrap());
        assert_eq!(MSG1, recv.reassemble_frag(now(), f1).unwrap().unwrap());
        assert_eq!(MSG2, recv.reassemble_frag(now(), f2).unwrap().unwrap());
    }

    #[test]
    fn large1() {
        let (send, mut recv) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN + 10));
        let [f1, f2] = send
            .fragment(MessageSeq::new(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(recv.reassemble_frag(now(), f1), Ok(None));
        assert_matches!(recv.reassemble_frag(now(), f2), Ok(Some(b)) if b == msg);
    }

    #[test]
    fn large2() {
        let (send, mut recv) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN * 2 + 10));
        let [f1, f2, f3] = send
            .fragment(MessageSeq::new(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(recv.reassemble_frag(now(), f1), Ok(None));
        assert_matches!(recv.reassemble_frag(now(), f2), Ok(None));
        assert_matches!(recv.reassemble_frag(now(), f3), Ok(Some(b)) if b == msg);
    }
}
