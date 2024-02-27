use std::{
    num::NonZeroU8,
    time::{Duration, Instant},
};

use arbitrary::Arbitrary;
use bitvec::{bitvec, vec::BitVec};

use crate::Seq;

/// Metadata for a packet produced by [`Fragmentation::fragment`] and read by
/// [`Fragmentation::reassemble`].
#[derive(Debug, Clone, Arbitrary)]
pub struct FragmentHeader {
    /// How many fragments this packet's message is split up into.
    pub num_frags: NonZeroU8,
    /// Index of this fragment in the total message.
    pub frag_id: u8,
}

impl FragmentHeader {
    /// [Encoded](FragmentHeader::encode) size of this value in bytes.
    pub const SIZE: usize = 1 + 1;

    /// Encodes this value into a byte buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is too short to encode this.
    pub fn encode(&self, buf: &mut octets::OctetsMut<'_>) -> octets::Result<()> {
        buf.put_u8(self.num_frags.get())?;
        buf.put_u8(self.frag_id)?;
        Ok(())
    }

    /// Decodes this value from a byte buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is too short to decode this.
    pub fn decode(buf: &mut octets::Octets<'_>) -> octets::Result<Option<Self>> {
        let num_frags = match NonZeroU8::new(buf.get_u8()?) {
            Some(num_frags) => num_frags,
            None => return Ok(None),
        };
        let frag_id = buf.get_u8()?;
        Ok(Some(Self { num_frags, frag_id }))
    }
}

/// Error that occurs when using [`Fragmentation::fragment`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum FragmentError {
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
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReassembleError {
    /// Fragment ID was not valid for the current message being processed.
    ///
    /// This indicates that the peer sent us invalid data, either maliciously
    /// or it was corrupted in transit.
    #[error("invalid fragment id {frag_id}")]
    InvalidFragId {
        /// ID of the fragment.
        frag_id: u8,
    },
    /// The stored sequence number in the message's buffer was different to
    /// what the fragment's header reported.
    ///
    /// This can happen if the fragment is being received incredibly late, and
    /// we attempt to place it into a message buffer which is already occupied
    /// by an existing message.
    #[error("mismatched sequence number - local: {}, received: {}", local.0, recv.0)]
    MismatchedSeq {
        /// Sequence number in our local message buffer.
        local: Seq,
        /// Sequence number in the fragment header.
        recv: Seq,
    },
    /// The stored number of fragments in the message's buffer was different to
    /// what the fragment's header reported.
    ///
    /// This can happen if the fragment is being received incredibly late, and
    /// we attempt to place it into a message buffer which is already occupied
    /// by an existing message.
    #[error("mismatched number of fragments - local: {}, received: {}", local.get(), recv.get())]
    MismatchedNumFrags {
        /// Number of fragments in our local message buffer.
        local: NonZeroU8,
        /// Number of fragments in the fragment header.
        recv: NonZeroU8,
    },
    /// The fragment for the given ID was already received.
    ///
    /// This indicates that the same packet was received twice, possibly due
    /// to duplication in the network.
    #[error("already received this fragment")]
    AlreadyReceived,
    /// The fragment is not the last fragment in the message, but its length was
    /// not equal to [`FragmentationConfig::payload_size`].
    ///
    /// This can happen if the packet is extended in transit.
    #[error("invalid payload length - length: {len}, expected: {expect}")]
    InvalidPayloadLength {
        /// Size of the payload received.
        len: usize,
        /// Exact size that the payload was expected to be.
        expect: usize,
    },
    /// The last fragment for the given message is too large.
    ///
    /// This can happen if the packet is extended in transit.
    #[error("last fragment is too large - length: {len}, max: {max}")]
    LastFragTooLarge {
        /// Size of the payload received.
        len: usize,
        /// Maximum size that the last fragment's payload can be.
        max: usize,
    },
}

/// Handles splitting and reassembling a single large message into multiple
/// smaller packets for sending over a network.
#[derive(Debug)]
pub struct Fragmentation {
    payload_size: usize,
    messages: Vec<Option<MessageBuffer>>,
}

impl Fragmentation {
    /// Creates a new fragmentation sender/receiver from the given
    /// configuration.
    ///
    /// * `payload_size` defines the maximum size, in bytes, that the payload
    ///   of a single fragmented packet can be. This must be greater than 0.
    /// * `msg_buf_size` defines the maximum number of messages which can be
    ///   buffered at once. Attempting to [reassemble] any more messages will
    ///   result in [`ReassembleError`]s.
    ///
    /// # Panics
    ///
    /// Panics if `payload_size` is 0.
    ///
    /// [reassemble]: Fragmentation::reassemble
    pub fn new(payload_size: usize, msg_buf_size: usize) -> Self {
        assert!(payload_size > 0);
        Self {
            payload_size,
            messages: (0..msg_buf_size).map(|_| None).collect(),
        }
    }
}

#[derive(Debug)]
struct MessageBuffer {
    seq: Seq,
    num_frags: NonZeroU8,
    num_frags_recv: u8,
    recv_frags: BitVec,
    payload: Vec<u8>,
    last_recv_at: Instant,
}

impl MessageBuffer {
    fn new(payload_size: usize, seq: Seq, header: &FragmentHeader) -> Self {
        Self {
            // the sequence number isn't *strictly* required for reassembly to
            // work, but it provides a nice guarantee that we're reassembling
            // the right packet, since we're using a rolling buffer of messages
            seq,
            num_frags: header.num_frags,
            num_frags_recv: 0,
            // use a (BitVec, Vec<u8>) instead of a Vec<Option<u8>>
            // for efficiency
            recv_frags: bitvec![0; usize::from(header.num_frags.get())],
            // initially, we allocate space assuming that each packet received
            // will contain `payload_len` bytes of payload data.
            // in practice, the last payload received will be smaller than
            // `payload_size` - the receiving code takes care of resizing the
            // byte vec appropriately.
            // we could store this as a `Vec<Vec<u8>>` instead, but nah
            // it would cost more on the final packet reassemble
            payload: vec![0; usize::from(header.num_frags.get()) * payload_size],
            last_recv_at: Instant::now(),
        }
    }
}

#[derive(Debug)]
pub struct FragmentData<'a> {
    pub header: FragmentHeader,
    pub payload: &'a [u8],
}

impl Fragmentation {
    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// # Errors
    ///
    /// Errors if the message was not a valid message which could be fragmented.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment<'a>(
        &self,
        msg: &'a [u8],
    ) -> Result<impl Iterator<Item = FragmentData<'a>> + 'a, FragmentError> {
        let chunks = msg.chunks(self.payload_size);
        let num_frags = NonZeroU8::new(u8::try_from(chunks.len()).map_err(|_| {
            FragmentError::MessageTooBig {
                len: msg.len(),
                max: usize::from(u8::MAX) * self.payload_size,
            }
        })?)
        .ok_or(FragmentError::EmptyMessage)?;

        Ok(chunks.enumerate().map(move |(frag_id, payload)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let header = FragmentHeader { num_frags, frag_id };
            FragmentData { header, payload }
        }))
    }

    /// Receives a fragmented packet and attempts to reassemble this fragment
    /// into a message.
    ///
    /// You must parse the sequence number and header of the packet yourself
    /// and provide them to this function.
    ///
    /// # Errors
    ///
    /// Errors if the message could not be reassembled properly.
    ///
    /// It is perfectly safe to ignore these errors - they are provided more
    /// for clarity on why reassembly failed, rather than a fatal error
    /// condition for a connection.
    ///
    /// If this returns `Ok(Some(..))`, the resulting bytes will be the fully
    /// reassembled bytes of the message.
    ///
    /// Note that the returned [`Vec`] may not have an equal length and
    /// capacity - if you want to convert this into e.g. a [`bytes::Bytes`],
    /// there may be a reallocation involved.
    pub fn reassemble(
        &mut self,
        seq: Seq,
        packet: &FragmentData<'_>,
    ) -> Result<Option<Vec<u8>>, ReassembleError> {
        let FragmentData { header, payload } = packet;
        if header.num_frags.get() == 1 {
            // quick path to avoid writing this into the message buffer then
            // immediately reading it back out
            return Ok(Some(payload.to_vec()));
        }

        let buf_index = usize::from(seq.0) % self.messages.len();
        let buf_opt = &mut self.messages[buf_index];
        let buf = buf_opt.get_or_insert_with(|| MessageBuffer::new(self.payload_size, seq, header));

        // make sure that `buf` really does point to the same message that we're
        // meant to be reassembling
        if buf.seq != seq {
            return Err(ReassembleError::MismatchedSeq {
                local: buf.seq,
                recv: seq,
            });
        }
        if buf.num_frags != header.num_frags {
            return Err(ReassembleError::MismatchedNumFrags {
                local: buf.num_frags,
                recv: header.num_frags,
            });
        }

        // mark this fragment as received
        let frag_id = usize::from(header.frag_id);
        let mut is_received =
            buf.recv_frags
                .get_mut(frag_id)
                .ok_or(ReassembleError::InvalidFragId {
                    frag_id: header.frag_id,
                })?;
        if *is_received {
            return Err(ReassembleError::AlreadyReceived);
        }
        *is_received = true;
        drop(is_received); // otherwise `buf` is never dropped

        // and copy it into the payload buffer
        let is_last_frag = header.frag_id == buf.num_frags.get() - 1;
        let (start, end) = if is_last_frag {
            // resize the buffer down to fit this last payload
            let len = usize::from(header.num_frags.get() - 1) * self.payload_size + payload.len();
            if len > buf.payload.len() {
                // can't shrink the buffer to a larger amount,
                // that makes no sense
                return Err(ReassembleError::LastFragTooLarge {
                    len: payload.len(),
                    max: self.payload_size,
                });
            }
            // note: explicitly don't mess with the capacity, to avoid reallocs
            // the caller can realloc if they want to, but we don't
            buf.payload.truncate(len);

            let frag_id = usize::from(header.frag_id);
            (
                frag_id * self.payload_size,
                frag_id * self.payload_size + payload.len(),
            )
        } else {
            if payload.len() != self.payload_size {
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expect: self.payload_size,
                });
            }

            let frag_id = usize::from(header.frag_id);
            (
                frag_id * self.payload_size,
                (frag_id + 1) * self.payload_size,
            )
        };
        buf.payload[start..end].copy_from_slice(payload);

        // update stats
        buf.num_frags_recv += 1;
        buf.last_recv_at = Instant::now();

        debug_assert!(buf.num_frags_recv <= buf.num_frags.get());
        if buf.num_frags_recv == buf.num_frags.get() {
            // we've received all fragments for this message
            // return the fragment to the user
            let msg = std::mem::take(&mut buf.payload);
            *buf_opt = None;
            Ok(Some(msg))
        } else {
            // this message isn't complete yet, nothing to return
            Ok(None)
        }
    }

    /// Drops any messages which have not recently received any new fragments.
    ///
    /// The threshold for "recently" is defined by `drop_after`.
    pub fn clean_up(&mut self, drop_after: Duration) {
        let now = Instant::now();
        for opt_buf in self.messages.iter_mut() {
            if let Some(buf) = opt_buf {
                if now - buf.last_recv_at >= drop_after {
                    *opt_buf = None;
                }
            }
        }
    }

    /// Drops all currently buffered messages.
    pub fn clear(&mut self) {
        for buf in self.messages.iter_mut() {
            *buf = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    const PAYLOAD_SIZE: usize = 1024;
    const NUM_BUFFERD_MSGS: usize = 8;

    const MSG1: &[u8] = b"Message 1";
    const MSG2: &[u8] = b"Message 2";
    const MSG3: &[u8] = b"Message 3";

    fn frag() -> Fragmentation {
        Fragmentation::new(PAYLOAD_SIZE, NUM_BUFFERD_MSGS)
    }

    #[test]
    fn single_in_order() {
        let mut frag = frag();
        let p1 = frag.fragment(MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment(MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment(MSG3).unwrap().next().unwrap();
        assert_eq!(MSG1, frag.reassemble(Seq(0), &p1).unwrap().unwrap());
        assert_eq!(MSG2, frag.reassemble(Seq(1), &p2).unwrap().unwrap());
        assert_eq!(MSG3, frag.reassemble(Seq(2), &p3).unwrap().unwrap());
    }

    #[test]
    fn single_out_of_order() {
        let mut frag = frag();
        let p1 = frag.fragment(MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment(MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment(MSG3).unwrap().next().unwrap();
        assert_eq!(MSG3, frag.reassemble(Seq(2), &p3).unwrap().unwrap());
        assert_eq!(MSG1, frag.reassemble(Seq(0), &p1).unwrap().unwrap());
        assert_eq!(MSG2, frag.reassemble(Seq(1), &p2).unwrap().unwrap());
    }

    #[test]
    fn large1() {
        let mut frag = frag();
        let msg = b"x".repeat(PAYLOAD_SIZE + 1);
        let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(Seq(0), &packets[0]), Ok(None));
        assert_eq!(msg, frag.reassemble(Seq(0), &packets[1]).unwrap().unwrap());
    }

    #[test]
    fn large2() {
        let mut frag = frag();
        let msg = b"x".repeat(PAYLOAD_SIZE * 2 + 1);
        let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
        assert_eq!(3, packets.len());
        assert_matches!(frag.reassemble(Seq(0), &packets[0]), Ok(None));
        assert_matches!(frag.reassemble(Seq(0), &packets[1]), Ok(None));
        assert_eq!(msg, frag.reassemble(Seq(0), &packets[2]).unwrap().unwrap());
    }

    #[test]
    fn overflow_with_complete_messages() {
        let mut frag = frag();

        // since these are all completely reassembled messages, the message
        // buffer will be ready to receive new messages afterwards
        for seq in 0..NUM_BUFFERD_MSGS {
            let seq = u16::try_from(seq).unwrap();
            let msg = b"x".repeat(PAYLOAD_SIZE + 1);
            let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
            assert_eq!(2, packets.len());
            assert_matches!(frag.reassemble(Seq(seq), &packets[0]), Ok(None));
            assert_eq!(
                msg,
                frag.reassemble(Seq(seq), &packets[1]).unwrap().unwrap()
            );
        }

        let msg = b"x".repeat(PAYLOAD_SIZE + 1);
        let seq = Seq(u16::try_from(NUM_BUFFERD_MSGS).unwrap());
        let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(seq, &packets[0]), Ok(None));
        assert_eq!(msg, frag.reassemble(seq, &packets[1]).unwrap().unwrap());
    }

    #[test]
    fn overflow_with_incomplete_messages() {
        let mut frag = frag();

        for seq in 0..NUM_BUFFERD_MSGS {
            let seq = u16::try_from(seq).unwrap();
            let msg = b"x".repeat(PAYLOAD_SIZE + 1);
            let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
            assert_eq!(2, packets.len());
            assert_matches!(frag.reassemble(Seq(seq), &packets[0]), Ok(None));
            // we don't give it packets[1], so the message is not fully reassembled
        }

        let msg = b"x".repeat(PAYLOAD_SIZE + 1);
        let seq = Seq(u16::try_from(NUM_BUFFERD_MSGS).unwrap());

        // all the buffers will be full, so it can't reassemble this message
        let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(
            frag.reassemble(seq, &packets[0]),
            Err(ReassembleError::MismatchedSeq { .. })
        );
        assert_matches!(
            frag.reassemble(seq, &packets[1]),
            Err(ReassembleError::MismatchedSeq { .. })
        );

        // after clearing the buffers, we should be able to
        frag.clear();

        let packets = frag.fragment(&msg).unwrap().collect::<Vec<_>>();
        assert_eq!(2, packets.len());
        assert_matches!(frag.reassemble(seq, &packets[0]), Ok(None));
        assert_eq!(msg, frag.reassemble(seq, &packets[1]).unwrap().unwrap());
    }
}
