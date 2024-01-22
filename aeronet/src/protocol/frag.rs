use std::{array, mem, time::{Duration, Instant}};

use bitcode::{Decode, Encode};

use super::Seq;

/// Metadata for a [`Fragmentation`] packet.
#[derive(Debug, Clone, Encode, Decode)]
struct PacketHeader {
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
pub const MAX_PACKET_SIZE: usize = 1024;

/// Size of [`PacketHeader`] both in raw bytes in memory, and the byte size as
/// output by [`bitcode::encode`].
/// 
/// These two sizes must *always* be the same - this is checked through
/// `debug_assert`s.
const HEADER_SIZE: usize = mem::size_of::<PacketHeader>();

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

/// After a message has not received a new fragment for this duration, it will
/// be cleaned up.
/// 
/// Since fragments may never be delivered to the receiver, the receiver may be
/// stuck waiting for fragments to complete a message that the sender will never
/// send out again. This will eventually consume all the slots in the message
/// buffer, preventing any new messages from being received. Automatic clean-up
/// prevents this issue.
const CLEAN_UP_AFTER: Duration = Duration::from_secs(3);

/// Error that occurs when using [`Fragmentation`] for packet fragmentation
/// and reassembly.
///
/// Note that only conditions which are caused by a sender/receiver sending
/// invalid *data* is considered an error; sending valid *data* but at an
/// invalid *time* (i.e. receiving a packet 20 minutes after it was sent) is not
/// considered an error.
///
/// Errors during receiving may be safely ignored - they won't corrupt the state
/// of the fragmentation system - or they can be bubbled up. Up to you.
#[derive(Debug, thiserror::Error)]
pub enum FragmentationError {
    /// Attempted to send a message which was too big.
    #[error("message too big; {len} / {MAX_MESSAGE_SIZE} bytes")]
    MessageTooBig {
        /// Size of the message in bytes.
        len: usize,
    },
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

#[derive(Debug)]
pub struct Fragmentation {
    seq: Seq,
    // Instead of storing like a `Option<MessageBuffer>` for each element, which
    // would allow us a more "type-safe" test for if a certain message slot
    // actually contains a message, we can just say that certain values in
    // MessageBuffer are invalid, and therefore represent a free slot -
    // see MessageBuffer::is_occupied.
    // This is done to save memory.
    messages: Box<[MessageBuffer; MESSAGES_BUF as usize]>,
}

impl Default for Fragmentation {
    fn default() -> Self {
        Self {
            seq: Seq::default(),
            messages: Box::new(array::from_fn(|_| MessageBuffer::default())),
        }
    }
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

impl Fragmentation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clean_up(&mut self) {
        for buf in self.messages.iter_mut() {
            if buf.is_occupied() && buf.last_recv_at.elapsed() > CLEAN_UP_AFTER {
                buf.free();
            }
        }
    }

    pub fn force_clean_up(&mut self) {
        for buf in self.messages.iter_mut() {
            buf.free();
        }
    }

    // TODO: I really don't like the fact that we allocate a whole new Vec here
    // can't we chain iterators or somehow?
    pub fn fragment<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Result<impl Iterator<Item = Vec<u8>> + 'a, FragmentationError> {
        let seq = self.seq.next();

        let chunks = bytes.chunks(MAX_PAYLOAD_SIZE);
        let num_frags = u8::try_from(chunks.len())
            .map_err(|_| FragmentationError::MessageTooBig { len: bytes.len() })?;

        Ok(chunks.enumerate().map(move |(frag_id, chunk)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let header = PacketHeader {
                seq,
                frag_id,
                num_frags,
            };
            let mut packet = bitcode::encode(&header)
                .expect("does not use #[bitcode(with_serde)], so should never fail");
            debug_assert_eq!(HEADER_SIZE, packet.len());

            packet.reserve_exact(MAX_PAYLOAD_SIZE.min(chunk.len()));
            packet.extend(chunk);
            debug_assert!(packet.len() <= MAX_PACKET_SIZE);

            packet
        }))
    }

    pub fn reassemble(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>, FragmentationError> {
        if packet.len() < HEADER_SIZE {
            return Err(FragmentationError::PacketTooSmall { len: packet.len() });
        }
        if packet.len() > MAX_PACKET_SIZE {
            return Err(FragmentationError::PacketTooBig { len: packet.len() });
        }

        let header = bitcode::decode::<PacketHeader>(&packet[..HEADER_SIZE])
            .map_err(FragmentationError::DecodeHeader)?;
        let payload = &packet[HEADER_SIZE..];

        self.reassemble_packet(header, payload)
    }

    fn reassemble_packet(
        &mut self,
        header: PacketHeader,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, FragmentationError> {
        match header.num_frags {
            0 => Err(FragmentationError::InvalidHeader),
            // quick path to avoid writing this into the packet buffer then
            // immediately reading it back out
            1 => Ok(Some(payload.to_vec())),
            _ => Ok(self.reassemble_fragment(header, payload)),
        }
    }

    fn reassemble_fragment(&mut self, header: PacketHeader, payload: &[u8]) -> Option<Vec<u8>> {
        let buf = &mut self.messages[header.seq.0 as usize % MESSAGES_BUF];
        if !buf.is_occupied() {
            // let's initialize it
            buf.occupy(header.seq, header.num_frags);
        }

        // make sure that `buf` really does point to the same message that we're
        // meant to be reassembling
        if buf.seq != header.seq {
            return None;
        }
        if buf.num_frags != header.num_frags {
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

// TODO these tests need to be cleaned up, they're more of just a playground right now
#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn small() {
        let mut buf: VecDeque<Vec<u8>> = VecDeque::new();
        let mut frag = Fragmentation::new();

        // send
        let msg = b"Hello world!";
        buf.extend(frag.fragment(msg).unwrap());

        // recv
        assert_eq!(
            msg.as_slice(),
            frag.reassemble(&buf.pop_front().unwrap())
                .unwrap()
                .unwrap()
                .as_slice()
        );
    }

    #[test]
    fn big() {
        let mut buf: VecDeque<Vec<u8>> = VecDeque::new();
        let mut frag = Fragmentation::new();

        // send
        let msg = format!("Hello world! {}", "abcd".repeat(700));
        buf.extend(frag.fragment(msg.as_bytes()).unwrap());

        // recv
        assert!(matches!(
            frag.reassemble(&buf.pop_front().unwrap()),
            Ok(None)
        ));
        assert!(matches!(
            frag.reassemble(&buf.pop_front().unwrap()),
            Ok(None)
        ));
        assert_eq!(
            msg.as_bytes().to_vec(),
            frag.reassemble(&buf.pop_front().unwrap()).unwrap().unwrap(),
        );
    }

    #[test]
    fn out_of_order() {
        let mut buf: Vec<Vec<u8>> = Vec::new();
        let mut frag = Fragmentation::new();

        let msg = format!("Hello world! {}", "abcd".repeat(700));
        buf.extend(frag.fragment(msg.as_bytes()).unwrap());
        let packet1 = buf.pop().unwrap();
        let packet2 = buf.pop().unwrap();
        let packet3 = buf.pop().unwrap();

        assert!(matches!(frag.reassemble(&packet3), Ok(None)));
        assert!(matches!(frag.reassemble(&packet2), Ok(None)));
        assert_eq!(
            msg.as_bytes(),
            frag.reassemble(&packet1)
                .unwrap()
                .unwrap()
                .as_slice()
            );
    }

    #[test]
    fn overflow_with_complete_messages() {
        let mut frag = Fragmentation::new();

        // works because after we reassemble a packet, we clear the buffer slot
        for _ in 0..100 {
            const MSG: &[u8] = b"a";

            let packets = frag.fragment(MSG).unwrap().collect::<Vec<_>>();
            for packet in packets {
                assert_eq!(MSG, frag.reassemble(&packet).unwrap().unwrap().as_slice());
            }
        }

        const MSG: &[u8] = b"Hello world";

        let mut packets = frag.fragment(MSG).unwrap().collect::<Vec<_>>();
        assert_eq!(
            MSG,
            frag.reassemble(&packets.pop().unwrap())
                .unwrap()
                .unwrap()
                .as_slice()
        );
    }

    #[test]
    fn overflow_with_incomplete_messages() {
        let mut frag = Fragmentation::new();

        for seq in 0..100 {
            // add a bunch of incomplete messages, which are waiting for more
            // fragments
            let header = PacketHeader {
                seq: Seq(seq),
                frag_id: 0,
                // has to be different to the number of fragments that `msg`
                // below will take up
                num_frags: 10,
            };
            let _ = frag.reassemble_packet(header, &[]);
        }

        // try to get a new, complete, message
        // message has to be big to get fragmented to avoid the fast path
        let msg = format!("Hello world! {}", "abcd".repeat(300));
        let mut packets = frag.fragment(msg.as_bytes()).unwrap().collect::<Vec<_>>();

        // this will give us None, because there's no free fragments to put the
        // data in, but if we force clean up, it will be OK
        // (in real code, enough time will have elapsed for the fragments to be
        // cleared by the main app loop)
        frag.force_clean_up();

        println!(
            "{:?}",
            frag.reassemble(&packets.pop().unwrap())
                .map(|x| x.map(String::from_utf8))
        );
        println!(
            "{:?}",
            frag.reassemble(&packets.pop().unwrap())
                .map(|x| x.map(String::from_utf8))
        );
    }
}
