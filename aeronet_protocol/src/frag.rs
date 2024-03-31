//! See [`Fragmentation`].

use std::{
    iter::FusedIterator,
    num::NonZeroU8,
    time::{Duration, Instant},
};

use aeronet::{
    integer_encoding::VarInt,
    octs::{self, ByteChunks, ByteChunksExt, ConstEncodeSize},
};
use ahash::AHashMap;
use arbitrary::Arbitrary;
use bitvec::{array::BitArray, bitarr};
use bytes::{Buf, Bytes};

use crate::seq::Seq;

/// Handles splitting and reassembling a single large message into multiple
/// smaller packets for sending over a network.
///
/// # Memory management
///
/// The initial implementation used a fixed-size "sequence buffer" data
/// structure as proposed by [*Gaffer On Games*], however this is an issue when
/// we don't know how many fragments and messages we may be receiving, as this
/// buffer is able to run out of space. This current implementation, instead,
/// uses a map to store messages. This is able to grow infinitely, or at least
/// up to how much memory the computer has.
///
/// Due to the fact that fragments may be dropped in transport, and that old
/// messages waiting for more fragments to be received may never get those
/// fragments, users should be careful to clean up fragments periodically -
/// see [`Fragmentation::clean_up`].
///
/// [*Gaffer On Games*]: https://gafferongames.com/post/packet_fragmentation_and_reassembly/#data-structure-on-receiver-side
///
/// # Encoded layout
///
/// See [`FragmentHeader`].
#[derive(Debug)]
pub struct Fragmentation {
    payload_len: usize,
    messages: AHashMap<Seq, MessageBuffer>,
}

/// Error that occurs when using [`Fragmentation::fragment`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum FragmentError {
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
pub struct FragHeader {
    /// Sequence number of the message that this fragment is a part of.
    pub msg_seq: Seq,
    /// How many fragments this packet's message is split up into.
    pub num_frags: u8,
    /// Index of this fragment in the total message.
    pub frag_id: u8,
}

impl octs::ConstEncodeSize for FragHeader {
    const ENCODE_SIZE: usize = Seq::ENCODE_SIZE + u8::ENCODE_SIZE + u8::ENCODE_SIZE;
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

#[derive(Debug, Clone)]
struct MessageBuffer {
    num_frags: NonZeroU8,
    num_frags_recv: u8,
    recv_frags: BitArray<[u8; 32]>,
    payload: Vec<u8>,
    last_recv_at: Instant,
}

impl MessageBuffer {
    fn new(payload_len: usize, header: &FragHeader, num_frags: NonZeroU8) -> Self {
        Self {
            // use a NonZeroU8 because:
            // * having `num_frags = 0` is genuinely an invalid case
            // * allows niching in Option<MessageBuffer>
            //   * but I think this is stashed elsewhere -
            //     doesn't seem to change the size
            num_frags,
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
            payload: vec![0; usize::from(header.num_frags) * payload_len],
            last_recv_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment<B> {
    pub header: FragHeader,
    pub payload: B,
}

impl<B: Buf> Fragment<B> {
    // can't implement `Encode` because we need to consume the payload
    // and Encode takes a shared ref
    pub fn encode_into(mut self, buf: &mut impl octs::WriteBytes) -> octs::Result<()> {
        buf.write(&self.header)?;
        // if B is Bytes, this will be nearly free -
        // doesn't even increment the ref count
        let payload = self.payload.copy_to_bytes(self.payload.remaining());
        buf.write(&payload)?;
        Ok(())
    }
}

impl<B: Buf> octs::EncodeSize for Fragment<B> {
    fn encode_size(&self) -> usize {
        let len = self.payload.remaining();
        FragHeader::ENCODE_SIZE + VarInt::required_space(len) + len
    }
}

impl octs::Decode for Fragment<Bytes> {
    fn decode(buf: &mut impl octs::ReadBytes) -> octs::Result<Self> {
        Ok(Self {
            header: buf.read()?,
            payload: buf.read()?,
        })
    }
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
    pub fn new(payload_len: usize) -> Self {
        assert!(payload_len > 0);
        Self {
            payload_len,
            messages: AHashMap::new(),
        }
    }

    pub fn payload_len(&self) -> usize {
        self.payload_len
    }

    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// Returns an iterator over the individual fragments.
    ///
    /// * `msg_seq` represents the sequence of this specific message - note that
    ///   each fragment may be sent in a different packet with a different
    ///   packet sequence.
    /// * If `msg` is empty, this will return an empty iterator.
    ///
    /// # Errors
    ///
    /// Errors if the message was not a valid message which could be fragmented.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment<T>(&self, msg_seq: Seq, msg: T) -> Result<Fragments<T>, FragmentError>
    where
        T: Buf + ByteChunksExt,
        ByteChunks<T>: ExactSizeIterator,
    {
        let msg_len = msg.remaining();
        let chunks = msg.byte_chunks(self.payload_len);
        let num_frags = u8::try_from(chunks.len()).map_err(|_| FragmentError::MessageTooBig {
            len: msg_len,
            max: usize::from(u8::MAX) * self.payload_len,
        })?;

        Ok(Fragments {
            msg_seq,
            num_frags,
            iter: chunks.enumerate(),
        })
    }

    /// Receives a fragmented packet and attempts to reassemble this fragment
    /// into a message.
    ///
    /// You must parse the fragment header of the packet yourself and provide it
    /// to this function.
    ///
    /// If this returns `Ok(Some(..))`, the resulting bytes will be the fully
    /// reassembled bytes of the message.
    ///
    /// Note that the returned [`Vec`] may not have an equal length and
    /// capacity - if you want to convert this into e.g. a [`bytes::Bytes`],
    /// there may be a reallocation involved.
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
        header: &FragHeader,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, ReassembleError> {
        let num_frags = match NonZeroU8::new(header.num_frags) {
            // fast path since this fragment can't have any message anyway
            // this theoretically shouldn't happen with the default frag impl,
            // but a user may pass in `num_frags: 0` anyway
            None => return Ok(None),
            Some(num_frags) if num_frags.get() == 1 => {
                // fast path to avoid writing this into the message buffer then
                // immediately reading it back out
                return Ok(Some(payload.to_vec()));
            }
            Some(num_frags) => num_frags,
        };

        let buf = self
            .messages
            .entry(header.msg_seq)
            .or_insert_with(|| MessageBuffer::new(self.payload_len, header, num_frags));

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
        drop(is_received);

        // and copy it into the payload buffer
        let is_last_frag = header.frag_id == buf.num_frags.get() - 1;
        let (start, end) = if is_last_frag {
            // resize the buffer down to fit this last payload
            let len = usize::from(header.num_frags - 1) * self.payload_len + payload.len();
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
            let buf = self.messages.remove(&header.msg_seq).unwrap();
            Ok(Some(buf.payload))
        } else {
            // this message isn't complete yet, nothing to return
            Ok(None)
        }
    }

    /// Removes a message with the given sequence, dropping all its fragments.
    pub fn remove(&mut self, msg_seq: Seq) {
        self.messages.remove(&msg_seq);
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

#[derive(Debug)]
pub struct Fragments<T> {
    msg_seq: Seq,
    num_frags: u8,
    iter: std::iter::Enumerate<octs::ByteChunks<T>>,
}

impl<T> Fragments<T> {
    pub fn num_frags(&self) -> u8 {
        self.num_frags
    }
}

impl<T, U> Iterator for Fragments<T>
where
    ByteChunks<T>: Iterator<Item = U>,
{
    type Item = Fragment<U>;

    fn next(&mut self) -> Option<Self::Item> {
        let (frag_id, payload) = self.iter.next()?;
        let frag_id =
            u8::try_from(frag_id).expect("`num_frags` is a u8, so `frag_id` should be convertible");
        let header = FragHeader {
            msg_seq: self.msg_seq,
            num_frags: self.num_frags,
            frag_id,
        };
        Some(Fragment { header, payload })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T, U> ExactSizeIterator for Fragments<T> where ByteChunks<T>: ExactSizeIterator<Item = U> {}

impl<T, U> FusedIterator for Fragments<T> where ByteChunks<T>: FusedIterator<Item = U> {}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use bytes::BytesMut;

    use aeronet::octs::{ReadBytes, WriteBytes};

    use super::*;

    #[test]
    fn encode_decode_header() {
        let v = FragHeader {
            msg_seq: Seq(1),
            num_frags: 12,
            frag_id: 34,
        };
        let mut buf = BytesMut::with_capacity(FragHeader::ENCODE_SIZE);

        buf.write(&v).unwrap();
        assert_eq!(FragHeader::ENCODE_SIZE, buf.len());

        assert_eq!(v, buf.freeze().read::<FragHeader>().unwrap());
    }

    const PAYLOAD_SIZE: usize = 1024;

    const MSG1: Bytes = Bytes::from_static(b"Message 1");
    const MSG2: Bytes = Bytes::from_static(b"Message 2");
    const MSG3: Bytes = Bytes::from_static(b"Message 3");

    fn frag() -> Fragmentation {
        Fragmentation::new(PAYLOAD_SIZE)
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
        let msg = Bytes::from(b"x".repeat(PAYLOAD_SIZE + 1));
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
        let msg = Bytes::from(b"x".repeat(PAYLOAD_SIZE * 2 + 1));
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
