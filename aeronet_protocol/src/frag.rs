use std::{
    collections::BTreeMap,
    num::NonZeroU8,
    time::{Duration, Instant},
};

use arbitrary::Arbitrary;
use bitvec::{array::BitArray, bitarr};
use bytes::Bytes;

use crate::{bytes::ByteChunksExt, Seq};

/// Handles splitting and reassembling a single large message into multiple
/// smaller packets for sending over a network.
///
/// # Memory management
///
/// The initial implementation used a fixed-size "sequence buffer" data
/// structure as proposed by [*Gaffer On Games*], however this is an issue when
/// we don't know how many fragments and messages we may be receiving, as this
/// buffer is able to run out of space. This current implementation, instead,
/// uses a [`BTreeMap`] to store messages. This is able to grow infinitely, or
/// at least up to how much memory the computer has.
///
/// Due to the fact that fragments may be dropped in transport, and that old
/// messages waiting for more fragments to be received may never get those
/// fragments, users should be careful to clean up fragments periodically -
/// see [`Fragmentation::clean_up`].
///
/// [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#data-structure-on-receiver-side
#[derive(Debug)]
pub struct Fragmentation {
    payload_len: usize,
    messages: BTreeMap<Seq, MessageBuffer>,
}

impl Fragmentation {
    /// Creates a new fragmentation sender/receiver from the given
    /// configuration.
    ///
    /// * `payload_len` defines the maximum length, in bytes, that the payload
    ///   of a single fragmented packet can be. This must be greater than 0.
    ///
    /// # Panics
    ///
    /// Panics if `payload_len` is 0.
    ///
    /// [reassemble]: Fragmentation::reassemble
    pub fn new(payload_len: usize) -> Self {
        assert!(payload_len > 0);
        Self {
            payload_len,
            messages: BTreeMap::new(),
        }
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
        /// Length of the message in bytes.
        len: usize,
        /// Maximum length of the message in bytes.
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
    /// The fragment for the given ID was already received.
    ///
    /// This indicates that the same packet was received twice, possibly due
    /// to duplication in the network.
    #[error("already received this fragment")]
    AlreadyReceived,
    /// The fragment is not the last fragment in the message, but its length was
    /// not equal to [`FragmentationConfig::payload_len`].
    ///
    /// This can happen if the packet is extended in transit.
    #[error("invalid payload length - length: {len}, expected: {expect}")]
    InvalidPayloadLength {
        /// Length of the payload received.
        len: usize,
        /// Exact length that the payload was expected to be.
        expect: usize,
    },
    /// The last fragment for the given message is too large.
    ///
    /// This can happen if the packet is extended in transit.
    #[error("last fragment is too large - length: {len}, max: {max}")]
    LastFragTooLarge {
        /// Length of the payload received.
        len: usize,
        /// Maximum length that the last fragment's payload can be.
        max: usize,
    },
}

/// Metadata for a packet produced by [`Fragmentation::fragment`] and read by
/// [`Fragmentation::reassemble`].
#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct FragmentHeader {
    /// How many fragments this packet's message is split up into.
    pub num_frags: NonZeroU8,
    /// Index of this fragment in the total message.
    pub frag_id: u8,
}

impl FragmentHeader {
    /// [Encoded](FragmentHeader::encode) size of this value in bytes.
    pub const ENCODE_SIZE: usize = 1 + 1;

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
        let num_frags = buf.get_u8()?;
        let frag_id = buf.get_u8()?;
        let num_frags = match NonZeroU8::new(num_frags) {
            Some(num_frags) => num_frags,
            None => return Ok(None),
        };
        Ok(Some(Self { num_frags, frag_id }))
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

impl MessageBuffer {
    fn new(payload_len: usize, header: &FragmentHeader) -> Self {
        Self {
            // use a NonZeroU8 because:
            // * having `num_frags = 0` is genuinely an invalid case
            // * allows niching in Option<MessageBuffer>
            //   * but I think this is stashed in the padding anyway - doesn't
            //     seem to change the size
            num_frags: header.num_frags,
            num_frags_recv: 0,
            // use a (BitArray, Vec<u8>) instead of a Vec<Option<u8>>
            // for efficiency
            recv_frags: bitarr![u8, bitvec::order::Lsb0; 0; 256],
            // initially, we allocate space assuming that each packet received
            // will contain `payload_len` bytes of payload data.
            // in practice, the last payload received will be smaller than
            // `payload_len` - the receiving code takes care of resizing the
            // byte vec appropriately.
            // we could store this as a `Vec<Vec<u8>>` instead, but nah
            // it would cost more on the final packet reassemble
            payload: vec![0; usize::from(header.num_frags.get()) * payload_len],
            last_recv_at: Instant::now(),
        }
    }
}

#[derive(Debug)]
pub struct FragmentSliceData<'a> {
    pub header: FragmentHeader,
    pub payload: &'a [u8],
}

#[derive(Debug)]
pub struct FragmentBytesData {
    pub header: FragmentHeader,
    pub payload: Bytes,
}

impl Fragmentation {
    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// This version accepts [`Bytes`] and creates cheap clones of the
    /// underlying bytes object for each fragment.
    ///
    /// # Errors
    ///
    /// Errors if the message was not a valid message which could be fragmented.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment_bytes(
        &self,
        msg: Bytes,
    ) -> Result<impl Iterator<Item = FragmentBytesData> + '_, FragmentError> {
        let msg_len = msg.len();
        let chunks = msg.byte_chunks(self.payload_len);
        let num_frags = NonZeroU8::new(u8::try_from(chunks.len()).map_err(|_| {
            FragmentError::MessageTooBig {
                len: msg_len,
                max: usize::from(u8::MAX) * self.payload_len,
            }
        })?)
        .ok_or(FragmentError::EmptyMessage)?;

        Ok(chunks.enumerate().map(move |(frag_id, payload)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let header = FragmentHeader { num_frags, frag_id };
            FragmentBytesData { header, payload }
        }))
    }

    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// This version accepts any byte slice and creates fragments whose lifetime
    /// is tied to the original byte slice.
    ///
    /// # Errors
    ///
    /// Errors if the message was not a valid message which could be fragmented.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment_slice<'a>(
        &self,
        msg: &'a [u8],
    ) -> Result<impl Iterator<Item = FragmentSliceData<'a>> + 'a, FragmentError> {
        let chunks = msg.chunks(self.payload_len);
        let num_frags = NonZeroU8::new(u8::try_from(chunks.len()).map_err(|_| {
            FragmentError::MessageTooBig {
                len: msg.len(),
                max: usize::from(u8::MAX) * self.payload_len,
            }
        })?)
        .ok_or(FragmentError::EmptyMessage)?;

        Ok(chunks.enumerate().map(move |(frag_id, payload)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let header = FragmentHeader { num_frags, frag_id };
            FragmentSliceData { header, payload }
        }))
    }

    /// Receives a fragmented packet and attempts to reassemble this fragment
    /// into a message.
    ///
    /// You must parse the sequence number and header of the packet yourself
    /// and provide them to this function.
    ///
    /// If this returns `Ok(Some(..))`, the resulting bytes will be the fully
    /// reassembled bytes of the message.
    ///
    /// Note that the returned [`Vec`] may not have an equal length and
    /// capacity - if you want to convert this into e.g. a [`bytes::Bytes`],
    /// there may be a reallocation involved.
    ///
    /// This function will internally copy the payload byte slice into its own
    /// storage, so creating an alternate function taking [`Bytes`] will not
    /// help performance or allocations.
    ///
    /// # Errors
    ///
    /// Errors if the message could not be reassembled properly.
    ///
    /// It is perfectly safe to ignore these errors - they are provided more
    /// for clarity on why reassembly failed, rather than a fatal error
    /// condition for a connection.
    pub fn reassemble(
        &mut self,
        seq: Seq,
        header: &FragmentHeader,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, ReassembleError> {
        if header.num_frags.get() == 1 {
            // quick path to avoid writing this into the message buffer then
            // immediately reading it back out
            return Ok(Some(payload.to_vec()));
        }

        let buf = self
            .messages
            .entry(seq)
            .or_insert_with(|| MessageBuffer::new(self.payload_len, header));

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
        // otherwise `buf` can't be dropped until the end
        drop(is_received);

        // and copy it into the payload buffer
        let is_last_frag = header.frag_id == buf.num_frags.get() - 1;
        let (start, end) = if is_last_frag {
            // resize the buffer down to fit this last payload
            let len = usize::from(header.num_frags.get() - 1) * self.payload_len + payload.len();
            if len > buf.payload.len() {
                // can't shrink the buffer to a larger amount,
                // that makes no sense
                return Err(ReassembleError::LastFragTooLarge {
                    len: payload.len(),
                    max: self.payload_len,
                });
            }
            // note: explicitly don't mess with the capacity, to avoid reallocs
            // the caller can realloc if they want to, but we don't
            buf.payload.truncate(len);

            let frag_id = usize::from(header.frag_id);
            (
                frag_id * self.payload_len,
                frag_id * self.payload_len + payload.len(),
            )
        } else {
            if payload.len() != self.payload_len {
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expect: self.payload_len,
                });
            }

            let frag_id = usize::from(header.frag_id);
            (frag_id * self.payload_len, (frag_id + 1) * self.payload_len)
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
            self.messages.remove(&seq);
            Ok(Some(msg))
        } else {
            // this message isn't complete yet, nothing to return
            Ok(None)
        }
    }

    /// Drops any messages which have not recently received any new fragments.
    ///
    /// The threshold for "recently" is defined by `drop_after`.
    ///
    /// Returns the amount of messages removed.
    pub fn clean_up(&mut self, drop_after: Duration) -> usize {
        let now = Instant::now();
        let len_before = self.messages.len();
        self.messages
            .retain(|_, buf| now - buf.last_recv_at < drop_after);
        len_before - self.messages.len()
    }

    /// Drops all currently buffered messages.
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    #[test]
    fn encode_decode_header() {
        let header = FragmentHeader {
            num_frags: NonZeroU8::new(12).unwrap(),
            frag_id: 34,
        };
        let mut buf = [0; FragmentHeader::ENCODE_SIZE];

        let mut oct = octets::OctetsMut::with_slice(&mut buf);
        header.encode(&mut oct).unwrap();
        oct.peek_bytes(1).unwrap_err();

        let mut oct = octets::Octets::with_slice(&buf);
        assert_eq!(header, FragmentHeader::decode(&mut oct).unwrap().unwrap());
    }

    const PAYLOAD_SIZE: usize = 1024;

    const MSG1: &[u8] = b"Message 1";
    const MSG2: &[u8] = b"Message 2";
    const MSG3: &[u8] = b"Message 3";

    fn frag() -> Fragmentation {
        Fragmentation::new(PAYLOAD_SIZE)
    }

    #[test]
    fn single_in_order() {
        let mut frag = frag();
        let p1 = frag.fragment_slice(MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment_slice(MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment_slice(MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG1,
            frag.reassemble(Seq(0), &p1.header, &p1.payload)
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            MSG2,
            frag.reassemble(Seq(1), &p2.header, &p2.payload)
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            MSG3,
            frag.reassemble(Seq(2), &p3.header, &p3.payload)
                .unwrap()
                .unwrap()
        );
    }

    #[test]
    fn single_out_of_order() {
        let mut frag = frag();
        let p1 = frag.fragment_slice(MSG1).unwrap().next().unwrap();
        let p2 = frag.fragment_slice(MSG2).unwrap().next().unwrap();
        let p3 = frag.fragment_slice(MSG3).unwrap().next().unwrap();
        assert_eq!(
            MSG3,
            frag.reassemble(Seq(2), &p3.header, &p3.payload)
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            MSG1,
            frag.reassemble(Seq(0), &p1.header, &p3.payload)
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            MSG2,
            frag.reassemble(Seq(1), &p2.header, &p3.payload)
                .unwrap()
                .unwrap()
        );
    }

    #[test]
    fn large1() {
        let mut frag = frag();
        let msg = b"x".repeat(PAYLOAD_SIZE + 1);
        let [p1, p2] = frag
            .fragment_slice(&msg)
            .unwrap()
            .collect::<Vec<_>>()
            .as_slice();
        assert_matches!(frag.reassemble(Seq(0), &p1.header, &p2.payload), Ok(None));
        assert_eq!(
            msg,
            frag.reassemble(Seq(0), &p2.header, &p2.payload)
                .unwrap()
                .unwrap()
        );
    }

    #[test]
    fn large2() {
        let mut frag = frag();
        let msg = b"x".repeat(PAYLOAD_SIZE * 2 + 1);
        let [p1, p2, p3] = frag
            .fragment_slice(&msg)
            .unwrap()
            .collect::<Vec<_>>()
            .as_slice();
        assert_matches!(frag.reassemble(Seq(0), &p1.header, &p1.payload), Ok(None));
        assert_matches!(frag.reassemble(Seq(0), &p2.header, &p2.payload), Ok(None));
        assert_eq!(
            msg,
            frag.reassemble(Seq(0), &p3.header, &p3.payload)
                .unwrap()
                .unwrap()
        );
    }
}
