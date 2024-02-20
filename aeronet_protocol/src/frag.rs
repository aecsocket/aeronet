use std::{
    marker::PhantomData,
    num::NonZeroU8,
    time::{Duration, Instant},
};

use bitcode::{Decode, Encode};
use bytes::Bytes;
use static_assertions::const_assert_eq;

use super::Seq;

/// Metadata for a [`Fragmentation`] packet.
#[derive(Debug, Clone, Encode, Decode)]
struct FragHeader {
    /// Sequence number of this packet's message.
    seq: Seq,
    /// Index of this fragment in the total message.
    frag_id: u8,
    /// How many fragments this packet's message is split up into.
    num_frags: NonZeroU8,
}

const_assert_eq!(FragHeader::ENCODE_MIN, FragHeader::ENCODE_MAX);

/// Encoded size of a fragmentation header in bytes.
pub const FRAG_HEADER_LEN: usize = FragHeader::ENCODE_MIN / 8;

/// Maximum number of fragmented messages which are tracked by the receiver.
///
/// When a fragment comes in with a new sequence number, it is tracked
/// internally in the messages buffer. Only a limited amount of these messages
/// can be tracked at once, and the limit is defined by this number.
const MESSAGES_BUF: usize = 256;

/// After a buffered message has not received a new fragment for this duration,
/// it will be cleaned up.
///
/// Since fragments may never be delivered to the receiver, the receiver may be
/// stuck waiting for fragments to complete a message that the sender will never
/// send out again. This will eventually consume all the slots in the message
/// buffer, preventing any new messages from being received. Automatic clean-up
/// prevents this issue.
const CLEAN_UP_AFTER: Duration = Duration::from_secs(3);

/// Error that occurs when using [`Fragmentation::fragment`].
#[derive(Debug, thiserror::Error)]
pub enum FragmentationError {
    /// Attempted to fragment a message with no bytes.
    #[error("empty message")]
    EmptyMessage,
    /// Attempted to fragment a message which was too big.
    #[error("message too big - {len} / {max} bytes")]
    MessageTooBig {
        /// Size of the message in bytes.
        len: usize,
        /// Maximum size of the message in bytes.
        max: usize,
    },
}

/// Error that occurs when using [`Fragmentation::reassemble`].
///
/// Note that only conditions which are caused by a sender/receiver sending
/// invalid *data* is considered an error; sending valid *data* but at an
/// invalid *time* (i.e. receiving a packet 20 minutes after it was sent) is not
/// considered an error.
///
/// Errors during reassembly may be safely ignored - they won't corrupt the state
/// of the fragmentation system - or they can be bubbled up. Up to you.
#[derive(Debug, thiserror::Error)]
pub enum ReassemblyError {
    /// Received a packet which was too small to contain header data.
    #[error("packet too small - {len} / {FRAG_HEADER_LEN} bytes")]
    PacketTooSmall {
        /// Size of the packet in bytes.
        len: usize,
    },
    /// Failed to decode a packet header.
    #[error("failed to decode packet header")]
    DecodeHeader(#[source] bitcode::Error),
    /// The decoded packet header contained invalid data.
    #[error("invalid packet header")]
    InvalidHeader,
}

/// Handles splitting and reassembling a single large message into multiple
/// smaller packets for sending over a network.
///
/// `S` is one of [`Sequenced`] or [`Unsequenced`].
///
/// See [`Fragmentation::fragment`] and [`Fragmentation::reassemble`].
#[derive(Debug)]
pub struct Fragmentation<S> {
    /// Next sequence number for outgoing messages.
    next_send_seq: Seq,
    /// Sequence number of the last message received, but not necessarily
    /// completed.
    ///
    /// Note that, as soon as the first fragment of a message is received, this
    /// value is updated to that fragment's sequence number. *Not* when the full
    /// message has been received.
    last_recv_seq: Seq,
    /// Buffer for incoming messages.
    messages: Box<[Option<MessageBuffer>; MESSAGES_BUF]>,
    _phantom: PhantomData<S>,
}

mod private {
    pub trait Sealed {}
}

/// How messages with incrementing sequence numbers are handled by [`Fragmentation`].
///
/// This is a *sealed trait*.
pub trait SequencingStrategy: private::Sealed {
    /// If this represents a sequenced strategy.
    const SEQUENCED: bool;
}

#[derive(Debug)]
pub struct Unsequenced;

impl private::Sealed for Unsequenced {}

impl SequencingStrategy for Unsequenced {
    const SEQUENCED: bool = false;
}

#[derive(Debug)]
pub struct Sequenced;

impl private::Sealed for Sequenced {}

impl SequencingStrategy for Sequenced {
    const SEQUENCED: bool = true;
}

#[derive(Debug)]
struct MessageBuffer {
    seq: Seq,
    num_frags: NonZeroU8,
    recv_frags: u8,
    last_recv_at: Instant,
    frags: Box<[Vec<u8>; u8::MAX as usize + 1]>,
}

impl MessageBuffer {
    fn new(header: &FragHeader) -> Self {
        Self {
            seq: header.seq,
            num_frags: header.num_frags,
            recv_frags: 0,
            last_recv_at: Instant::now(),
            frags: Box::new(std::array::from_fn(|_| Vec::default())),
        }
    }
}

impl<S> Default for Fragmentation<S> {
    fn default() -> Self {
        Self {
            next_send_seq: Seq(0),
            last_recv_seq: Seq(0),
            messages: Box::new(std::array::from_fn(|_| None)),
            _phantom: PhantomData,
        }
    }
}

impl Fragmentation<Unsequenced> {
    #[must_use]
    pub fn unsequenced() -> Self {
        Self::default()
    }
}

impl Fragmentation<Sequenced> {
    #[must_use]
    pub fn sequenced() -> Self {
        Self::default()
    }
}

/// Packet from [`Fragmentation::fragment`].
#[derive(Debug, Clone)]
pub struct FragmentedPacket<'a> {
    /// Bytes to add before the payload.
    pub header: Bytes,
    /// Original message payload which this fragment carries.
    pub payload: &'a [u8],
}

impl<S> Fragmentation<S> {
    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// `payload_size` specifies the size of the payload in each fragment
    /// returned in the iterator.
    ///
    /// This will increase the sequence number.
    ///
    /// # Errors
    ///
    /// Errors if the message was not a valid message which could be fragmented.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment<'a>(
        &mut self,
        msg: &'a [u8],
        payload_size: usize,
    ) -> Result<impl Iterator<Item = FragmentedPacket<'a>> + 'a, FragmentationError> {
        let seq = self.next_send_seq.get_and_increment();

        let chunks = msg.chunks(payload_size);
        let num_frags = NonZeroU8::new(u8::try_from(chunks.len()).map_err(|_| {
            FragmentationError::MessageTooBig {
                len: msg.len(),
                max: usize::from(u8::MAX) * payload_size,
            }
        })?)
        .ok_or(FragmentationError::EmptyMessage)?;

        Ok(chunks.enumerate().map(move |(frag_id, payload)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let frame = bitcode::encode(&FragHeader {
                seq,
                frag_id,
                num_frags,
            })
            .expect("does not use #[bitcode(with_serde)], so encoding should never fail");

            // ensures quick path in Bytes::from(Vec<u8>)
            debug_assert_eq!(frame.capacity(), frame.len());
            let header = Bytes::from(frame);

            FragmentedPacket { header, payload }
        }))
    }

    pub fn clean_up(&mut self) {
        for opt_buf in self.messages.iter_mut() {
            if let Some(buf) = opt_buf {
                if buf.last_recv_at.elapsed() > CLEAN_UP_AFTER {
                    *opt_buf = None;
                }
            }
        }
    }

    pub fn clear(&mut self) {
        for buf in self.messages.iter_mut() {
            *buf = None;
        }
    }
}

impl<S: SequencingStrategy> Fragmentation<S> {
    /// Receives a fragmented packet and attempts to reassemble this fragment
    /// into a message.
    ///
    /// If this returns `Ok(Some(..))`, the resulting bytes will be the fully
    /// reassembled bytes of the message.
    ///
    /// # Errors
    ///
    /// If the packet was malformed, this returns an error.
    pub fn reassemble(&mut self, packet: &[u8]) -> Result<Option<Bytes>, ReassemblyError> {
        if packet.len() < FRAG_HEADER_LEN {
            return Err(ReassemblyError::PacketTooSmall { len: packet.len() });
        }

        let header = bitcode::decode::<FragHeader>(&packet[..FRAG_HEADER_LEN])
            .map_err(ReassemblyError::DecodeHeader)?;
        let payload = &packet[FRAG_HEADER_LEN..];

        Ok(self.reassemble_packet(&header, payload))
    }

    fn reassemble_packet(&mut self, header: &FragHeader, payload: &[u8]) -> Option<Bytes> {
        if S::SEQUENCED && header.seq < self.last_recv_seq {
            return None;
        }
        self.last_recv_seq = header.seq;

        match header.num_frags.get() {
            // quick path to avoid writing this into the message buffer then
            // immediately reading it back out
            1 => Some(Bytes::from(payload.to_vec())),
            _ => self.reassemble_fragment(header, payload),
        }
    }

    fn reassemble_fragment(&mut self, header: &FragHeader, payload: &[u8]) -> Option<Bytes> {
        let buf_opt = &mut self.messages[header.seq.0 as usize % MESSAGES_BUF];
        let buf = buf_opt.get_or_insert_with(|| MessageBuffer::new(header));

        // make sure that `buf` really does point to the same message that we're
        // meant to be reassembling
        if buf.seq != header.seq || buf.num_frags != header.num_frags {
            return None;
        }

        let buf_payload = &mut buf.frags[usize::from(header.frag_id)];
        if !buf_payload.is_empty() {
            // we already got the payload for this fragment; is this a duplicate
            // packet?
            return None;
        }

        // add the payload
        buf_payload.extend(payload);
        buf.recv_frags += 1;
        buf.last_recv_at = Instant::now();

        if buf.recv_frags >= buf.num_frags.get() {
            // we've received all fragments for this message, collect them and
            // return the message
            let message = buf.frags[..usize::from(buf.num_frags.get())]
                .iter()
                .flatten()
                .copied()
                .collect();
            *buf_opt = None;
            Some(message)
        } else {
            // this message isn't complete yet, nothing to return
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    const MTU: usize = 1024;
    const MSG1: &[u8] = b"Message 1";
    const MSG2: &[u8] = b"Message 2";
    const MSG3: &[u8] = b"Message 3";

    fn b(packet: &FragmentedPacket<'_>) -> Vec<u8> {
        packet
            .header
            .iter()
            .chain(packet.payload)
            .copied()
            .collect::<Vec<_>>()
    }

    fn f<'a, 'b, S>(
        frag: &'a mut Fragmentation<S>,
        msg: &'b [u8],
    ) -> Result<impl Iterator<Item = FragmentedPacket<'b>>, FragmentationError> {
        frag.fragment(msg, MTU)
    }

    #[test]
    fn unsequenced_in_order() {
        let mut frag = Fragmentation::unsequenced();
        let packets1 = f(&mut frag, &MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = f(&mut frag, &MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = f(&mut frag, &MSG3).unwrap().collect::<Vec<_>>();
        assert_matches!(frag.reassemble(&b(&packets1[0])), Ok(Some(m)) if &m == MSG1);
        assert_matches!(frag.reassemble(&b(&packets2[0])), Ok(Some(m)) if &m == MSG2);
        assert_matches!(frag.reassemble(&b(&packets3[0])), Ok(Some(m)) if &m == MSG3);
    }

    #[test]
    fn sequenced_in_order() {
        let mut frag = Fragmentation::sequenced();
        let packets1 = f(&mut frag, &MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = f(&mut frag, &MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = f(&mut frag, &MSG3).unwrap().collect::<Vec<_>>();
        assert_matches!(frag.reassemble(&b(&packets1[0])), Ok(Some(m)) if &m == MSG1);
        assert_matches!(frag.reassemble(&b(&packets2[0])), Ok(Some(m)) if &m == MSG2);
        assert_matches!(frag.reassemble(&b(&packets3[0])), Ok(Some(m)) if &m == MSG3);
    }

    #[test]
    fn unsequenced_out_of_order() {
        let mut frag = Fragmentation::unsequenced();
        let packets1 = f(&mut frag, &MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = f(&mut frag, &MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = f(&mut frag, &MSG3).unwrap().collect::<Vec<_>>();
        assert_matches!(frag.reassemble(&b(&packets2[0])), Ok(Some(m)) if &m == MSG2);
        assert_matches!(frag.reassemble(&b(&packets1[0])), Ok(Some(m)) if &m == MSG1);
        assert_matches!(frag.reassemble(&b(&packets3[0])), Ok(Some(m)) if &m == MSG3);
    }

    #[test]
    fn sequenced_out_of_order() {
        let mut frag = Fragmentation::sequenced();
        let packets1 = f(&mut frag, &MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = f(&mut frag, &MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = f(&mut frag, &MSG3).unwrap().collect::<Vec<_>>();
        assert_matches!(frag.reassemble(&b(&packets2[0])), Ok(Some(m)) if &m == MSG2);
        assert_matches!(frag.reassemble(&b(&packets1[0])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets3[0])), Ok(Some(m)) if &m == MSG3);
    }

    #[test]
    fn large1() {
        let mut frag = Fragmentation::unsequenced();
        let msg = "x".repeat(MTU + 1);
        let packets = f(&mut frag, msg.as_bytes()).unwrap().collect::<Vec<_>>();

        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(&b(&packets[0])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets[1])), Ok(Some(m)) if &m == &msg);
    }

    #[test]
    fn large2() {
        let mut frag = Fragmentation::unsequenced();
        let msg = "x".repeat(MTU * 2 + 1);
        let packets = f(&mut frag, msg.as_bytes()).unwrap().collect::<Vec<_>>();

        assert_eq!(3, packets.len());
        assert_matches!(frag.reassemble(&b(&packets[0])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets[1])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets[2])), Ok(Some(m)) if &m == &msg);
    }

    #[test]
    fn overflow_with_complete_messages() {
        let mut frag = Fragmentation::unsequenced();

        // since these are all completely reassembled messages, the message
        // buffer will be ready to receive new messages afterwards
        for _ in 0..256 {
            let packets = f(&mut frag, &MSG1).unwrap().collect::<Vec<_>>();
            assert_matches!(frag.reassemble(&b(&packets[0])), Ok(Some(m)) if &m == MSG1);
        }

        let msg = "x".repeat(1024);
        let packets = f(&mut frag, msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(&b(&packets[0])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets[1])), Ok(Some(m)) if &m == &msg);
    }

    #[test]
    fn overflow_with_incomplete_messages() {
        let mut frag = Fragmentation::unsequenced();

        for _ in 0..256 {
            let msg = "x".repeat(1024);
            let packets = f(&mut frag, msg.as_bytes()).unwrap().collect::<Vec<_>>();
            assert_matches!(frag.reassemble(&b(&packets[0])), Ok(None));
            // crucially: we *don't* give it packets[1], so that the message is
            // in a partially complete state
        }

        let msg = "x".repeat(1024);
        // all the message buffers will be full, so we won't be able to
        // reassemble this message
        let packets = f(&mut frag, msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(&b(&packets[0])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets[1])), Ok(None));

        // but after cleanup, this *should* work
        frag.clear();

        let packets = f(&mut frag, msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(&b(&packets[0])), Ok(None));
        assert_matches!(frag.reassemble(&b(&packets[1])), Ok(Some(m)) if &m == &msg);
    }
}
