use std::{
    array,
    marker::PhantomData,
    mem,
    time::{Duration, Instant},
};

use bitcode::{Decode, Encode};
use bytes::Bytes;
use derivative::Derivative;

use super::Seq;

/// Metadata for a [`Fragmentation`] packet.
#[derive(Debug, Clone, Encode, Decode)]
struct FragHeader {
    /// Sequence number of this packet's message.
    seq: Seq,
    /// Index of this fragment in the total message.
    frag_id: u8,
    /// How many fragments this packet's message is split up into.
    num_frags: u8,
}

/// Maximum byte size of a single packet.
///
/// This value is a rough estimate of the MTU size for a typical internet
/// connection, with some allowance for e.g. VPNs. The maximum size of a
/// packet produced by [`Fragmentation`] will never be greater than this size.
#[doc(alias = "mtu")]
const MAX_PACKET_SIZE: usize = 1024;

/// Size of [`FragHeader`] both in raw bytes in memory, and the byte size as
/// output by [`bitcode::encode`].
///
/// These two sizes must *always* be the same - this is checked through
/// `debug_assert`s.
const HEADER_SIZE: usize = mem::size_of::<FragHeader>();

/// Maximum size of the user-defined payload sent in a single packet.
const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;

/// Maximum size of a user-defined message when it is sent fragmented.
///
/// A message can only be split up into a limited amount of fragments, so a
/// single message can only be as big as `MAX_PAYLOAD_SIZE * NUM_FRAGMENTS`.
const MAX_MESSAGE_SIZE: usize = MAX_PAYLOAD_SIZE * u8::MAX as usize;

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
    /// Attempted to send a message which was too big.
    #[error("message too big - {len} / {MAX_MESSAGE_SIZE} bytes")]
    MessageTooBig {
        /// Size of the message in bytes.
        len: usize,
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
    #[error("packet too small; {len} / {HEADER_SIZE} bytes")]
    PacketTooSmall {
        /// Size of the packet in bytes.
        len: usize,
    },
    /// Received a packet which was too big.
    #[error("packet too big; {len} / {MAX_PACKET_SIZE} bytes")]
    PacketTooBig {
        /// Size of the packet in bytes.
        len: usize,
    },
    /// Failed to decode a packet header.
    #[error("failed to decode packet header")]
    DecodeHeader(#[source] bitcode::Error),
    /// Decoded packet header contained invalid data.
    #[error("invalid packet header")]
    InvalidHeader,
}

/// Handles splitting and reassembling a single large message into multiple
/// smaller packets for sending over a network.
///
/// See [`Fragmentation::fragment`] and [`Fragmentation::reassemble`].
#[derive(Debug)]
pub struct Fragmentation {
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
    // Instead of storing like a `Option<MessageBuffer>` for each element, which
    // would allow us a more "type-safe" test for if a certain message slot
    // actually contains a message, we can just say that certain values in
    // MessageBuffer are invalid, and therefore represent a free slot -
    // see MessageBuffer::is_occupied.
    // This is done to save memory.
    messages: Box<[MessageBuffer; MESSAGES_BUF]>,
}

mod private {
    pub trait Sealed {}
}

/// How messages with incrementing sequence numbers are handled by [`Reassembly`].
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
    num_frags: u8,
    recv_frags: u8,
    last_recv_at: Instant,
    frags: Box<[Vec<u8>; u8::MAX as usize + 1]>,
}

impl Default for MessageBuffer {
    fn default() -> Self {
        Self {
            seq: Seq::default(),
            num_frags: u8::default(),
            recv_frags: u8::default(),
            last_recv_at: Instant::now(),
            frags: Box::new(array::from_fn(|_| Vec::default())),
        }
    }
}

impl MessageBuffer {
    fn is_occupied(&self) -> bool {
        self.num_frags > 0
    }

    fn occupy(&mut self, seq: Seq, num_frags: u8) {
        self.seq = seq;
        self.num_frags = num_frags;
    }

    fn free(&mut self) {
        self.num_frags = 0;
        self.recv_frags = 0;
        self.frags.fill(Vec::new());
    }
}

impl Default for Fragmentation {
    fn default() -> Self {
        Self {
            next_send_seq: Seq(0),
            last_recv_seq: Seq(0),
            messages: Box::new(array::from_fn(|_| MessageBuffer::default())),
        }
    }
}

impl Fragmentation {
    #[must_use]
    pub fn new() -> Self {
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

impl Fragmentation {
    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// This will increase the sequence number.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment<'a>(
        &'a mut self,
        msg: &'a [u8],
    ) -> Result<impl Iterator<Item = FragmentedPacket<'a>> + 'a, FragmentationError> {
        let seq = self.next_send_seq.next();

        let chunks = msg.chunks(MAX_PAYLOAD_SIZE);
        let num_frags = u8::try_from(chunks.len())
            .map_err(|_| FragmentationError::MessageTooBig { len: msg.len() })?;

        Ok(chunks.enumerate().map(move |(frag_id, payload)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let header = bitcode::encode(&FragHeader {
                seq,
                frag_id,
                num_frags,
            })
            .expect("does not use #[bitcode(with_serde)], so encoding should never fail");
            debug_assert_eq!(HEADER_SIZE, header.len());

            // ensures quick path in Bytes::from(Vec<u8>)
            debug_assert_eq!(header.capacity(), header.len());
            let header = Bytes::from(header);

            FragmentedPacket { header, payload }
        }))
    }

    pub fn clean_up(&mut self) {
        for buf in self.messages.iter_mut() {
            if buf.is_occupied() && buf.last_recv_at.elapsed() > CLEAN_UP_AFTER {
                buf.free();
            }
        }
    }

    pub fn clear(&mut self) {
        for buf in self.messages.iter_mut() {
            buf.free();
        }
    }

    /// Receives a fragmented packet and attempts to reassemble this fragment
    /// into a message.
    ///
    /// If this returns `Ok(Some(..))`, the resulting bytes will be the fully
    /// reassembled bytes of the message.
    ///
    /// Unsequenced: all messages will be received, regardless of if the
    /// received fragment has a lower sequence number than a previous fragment.
    ///
    /// # Errors
    ///
    /// If the packet was malformed, this returns an error.
    pub fn reassemble_unseq(&mut self, packet: &[u8]) -> Result<Option<Bytes>, ReassemblyError> {
        self.reassemble::<false>(packet)
    }

    /// Receives a fragmented packet and attempts to reassemble this fragment
    /// into a message.
    ///
    /// If this returns `Ok(Some(..))`, the resulting bytes will be the fully
    /// reassembled bytes of the message.
    ///
    /// Sequenced: a message will only be received if its fragment number is
    /// strictly lower than the latest sequence number received.
    ///
    /// # Errors
    ///
    /// If the packet was malformed, this returns an error.
    pub fn reassemble_seq(&mut self, packet: &[u8]) -> Result<Option<Bytes>, ReassemblyError> {
        self.reassemble::<true>(packet)
    }

    fn reassemble<const SEQUENCED: bool>(
        &mut self,
        packet: &[u8],
    ) -> Result<Option<Bytes>, ReassemblyError> {
        if packet.len() < HEADER_SIZE {
            return Err(ReassemblyError::PacketTooSmall { len: packet.len() });
        }
        if packet.len() > MAX_PACKET_SIZE {
            return Err(ReassemblyError::PacketTooBig { len: packet.len() });
        }

        let header = bitcode::decode::<FragHeader>(&packet[..HEADER_SIZE])
            .map_err(ReassemblyError::DecodeHeader)?;
        let payload = &packet[HEADER_SIZE..];

        self.reassemble_packet::<SEQUENCED>(header, payload)
    }

    fn reassemble_packet<const SEQUENCED: bool>(
        &mut self,
        header: FragHeader,
        payload: &[u8],
    ) -> Result<Option<Bytes>, ReassemblyError> {
        if SEQUENCED && header.seq < self.last_recv_seq {
            return Ok(None);
        }
        self.last_recv_seq = header.seq;

        match header.num_frags {
            0 => Err(ReassemblyError::InvalidHeader),
            // quick path to avoid writing this into the message buffer then
            // immediately reading it back out
            1 => Ok(Some(Bytes::from(payload.to_vec()))),
            _ => Ok(self.reassemble_fragment(&header, payload)),
        }
    }

    fn reassemble_fragment(&mut self, header: &FragHeader, payload: &[u8]) -> Option<Bytes> {
        let buf = &mut self.messages[header.seq.0 as usize % MESSAGES_BUF];
        if !buf.is_occupied() {
            // let's initialize it
            buf.occupy(header.seq, header.num_frags);
        }

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

        if buf.recv_frags >= buf.num_frags {
            // we've received all fragments for this message, collect them and
            // return the message
            let message = buf.frags[..usize::from(buf.num_frags)]
                .iter()
                .flatten()
                .copied()
                .collect();
            buf.free();
            Some(message)
        } else {
            // this message isn't complete yet, nothing to return
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSG1: &[u8] = b"Message 1";
    const MSG2: &[u8] = b"Message 2";
    const MSG3: &[u8] = b"Message 3";

    #[test]
    fn unsequenced_in_order() {
        let mut frag = Fragmentation::new();
        let packets1 = frag.fragment(&MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = frag.fragment(&MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = frag.fragment(&MSG3).unwrap().collect::<Vec<_>>();
        assert_eq!(MSG1, frag.reassemble_unseq(&packets1[0]).unwrap().unwrap());
        assert_eq!(MSG2, frag.reassemble_unseq(&packets2[0]).unwrap().unwrap());
        assert_eq!(MSG3, frag.reassemble_unseq(&packets3[0]).unwrap().unwrap());
    }

    #[test]
    fn sequenced_in_order() {
        let mut frag = Fragmentation::new();
        let packets1 = frag.fragment(&MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = frag.fragment(&MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = frag.fragment(&MSG3).unwrap().collect::<Vec<_>>();
        assert_eq!(MSG1, frag.reassemble_seq(&packets1[0]).unwrap().unwrap());
        assert_eq!(MSG2, frag.reassemble_seq(&packets2[0]).unwrap().unwrap());
        assert_eq!(MSG3, frag.reassemble_seq(&packets3[0]).unwrap().unwrap());
    }

    #[test]
    fn unsequenced_out_of_order() {
        let mut frag = Fragmentation::new();
        let packets1 = frag.fragment(&MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = frag.fragment(&MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = frag.fragment(&MSG3).unwrap().collect::<Vec<_>>();
        assert_eq!(MSG2, frag.reassemble_unseq(&packets2[0]).unwrap().unwrap());
        assert_eq!(MSG1, frag.reassemble_unseq(&packets1[0]).unwrap().unwrap());
        assert_eq!(MSG3, frag.reassemble_unseq(&packets3[0]).unwrap().unwrap());
    }

    #[test]
    fn sequenced_out_of_order() {
        let mut frag = Fragmentation::new();
        let packets1 = frag.fragment(&MSG1).unwrap().collect::<Vec<_>>();
        let packets2 = frag.fragment(&MSG2).unwrap().collect::<Vec<_>>();
        let packets3 = frag.fragment(&MSG3).unwrap().collect::<Vec<_>>();
        assert_eq!(MSG2, frag.reassemble_seq(&packets2[0]).unwrap().unwrap());
        assert!(matches!(frag.reassemble_seq(&packets1[0]), Ok(None)));
        assert_eq!(MSG3, frag.reassemble_seq(&packets3[0]).unwrap().unwrap());
    }

    #[test]
    fn large1() {
        let mut frag = Fragmentation::new();
        let msg = "x".repeat(1024);
        let packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert!(matches!(frag.reassemble_unseq(&packets[0]), Ok(None)));
        assert_eq!(msg, frag.reassemble_unseq(&packets[1]).unwrap().unwrap());
    }

    #[test]
    fn large2() {
        let mut frag = Fragmentation::new();
        let msg = "x".repeat(2048);
        let packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(3, packets.len());
        assert!(matches!(frag.reassemble_unseq(&packets[0]), Ok(None)));
        assert!(matches!(frag.reassemble_unseq(&packets[1]), Ok(None)));
        assert_eq!(msg, frag.reassemble_unseq(&packets[2]).unwrap().unwrap());
    }

    #[test]
    fn overflow_with_complete_messages() {
        let mut frag = Fragmentation::new();

        // since these are all completely reassembled messages, the message
        // buffer will be ready to receive new messages afterwards
        for _ in 0..256 {
            let packets = frag.fragment(&MSG1).unwrap().collect::<Vec<_>>();
            assert_eq!(MSG1, frag.reassemble_unseq(&packets[0]).unwrap().unwrap());
        }

        let msg = "x".repeat(1024);
        let packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert!(matches!(frag.reassemble_unseq(&packets[0]), Ok(None)));
        assert_eq!(msg, frag.reassemble_unseq(&packets[1]).unwrap().unwrap());
    }

    #[test]
    fn overflow_with_incomplete_messages() {
        let mut frag = Fragmentation::new();

        for _ in 0..256 {
            let msg = "x".repeat(1024);
            let packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();
            assert!(matches!(frag.reassemble_unseq(&packets[0]), Ok(None)));
            // crucially: we *don't* give it packets[1], so that the message is
            // in a partially complete state
        }

        let msg = "x".repeat(1024);
        // all the message buffers will be full, so we won't be able to
        // reassemble this message
        let packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert!(matches!(frag.reassemble_unseq(&packets[0]), Ok(None)));
        assert!(matches!(frag.reassemble_unseq(&packets[1]), Ok(None)));

        // but after cleanup, this *should* work
        frag.clear();

        let packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert!(matches!(frag.reassemble_unseq(&packets[0]), Ok(None)));
        assert_eq!(msg, frag.reassemble_unseq(&packets[1]).unwrap().unwrap());
    }
}
