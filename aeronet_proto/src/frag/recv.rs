use std::{collections::hash_map::Entry, num::NonZeroU8, time::Instant};

use ahash::AHashMap;
use bitvec::{array::BitArray, bitarr};
use bytes::Bytes;

use crate::{
    byte_count::{ByteBucket, ByteLimit, NotEnoughBytes},
    seq::Seq,
};

use super::FragmentHeader;

/// Handles reassembling small message fragments into one larger message which
/// was fragmented by a [`FragmentSender`].
///
/// [`FragmentSender`]: crate::frag::FragmentSender
#[derive(Debug, Clone)]
pub struct FragmentReceiver {
    max_payload_len: usize,
    messages: AHashMap<Seq, MessageBuffer>,
    bytes_left: ByteBucket,
}

#[derive(Debug, Clone)]
struct MessageBuffer {
    num_frags: NonZeroU8,
    num_frags_recv: u8,
    recv_frags: BitArray<[u8; 32]>,
    payload: Vec<u8>,
    last_recv_at: Instant,
}

/// Error that occurs when using [`FragmentReceiver::reassemble`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReassembleError {
    /// Fragment header states that this message has 0 fragments, which is invalid.
    #[error("number of fragments is 0")]
    NoFrags,
    /// Fragment index was not valid for the current message being processed.
    #[error("invalid fragment index")]
    InvalidFragIndex {
        /// Index of the fragment.
        frag_index: u8,
    },
    /// The fragment for the given index was already received.
    #[error("fragment already received")]
    AlreadyReceived,
    /// The fragment does not have the correct payload length.
    #[error("invalid payload length - length: {len}, expected: {expected}")]
    InvalidPayloadLength {
        /// Length of the payload received.
        len: usize,
        /// Exact length that the payload was expected to be.
        expected: usize,
    },
}

impl MessageBuffer {
    fn new(max_payload_len: usize, num_frags: NonZeroU8) -> Self {
        Self {
            // use a NonZeroU8 because:
            // * having `num_frags = 0` is genuinely an invalid case
            // * allows niching in Option<MessageBuffer>
            //   * but I think this is stashed elsewhere - doesn't seem to change the size
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
            payload: vec![0; usize::from(num_frags.get()) * max_payload_len],
            last_recv_at: Instant::now(),
        }
    }
}

impl FragmentReceiver {
    /// Creates a new [`FragmentReceiver`].
    ///
    /// * `max_payload_len` defines the maximum length, in bytes, that the
    ///   payload of a single fragmented packet can be. This must be greater
    ///   than 0.
    /// * `max_memory_usage` defines the maximum number of bytes that may be
    ///   used to store buffered messages. If you are not concerned with memory
    ///   usage, use [`usize::MAX`].
    ///
    /// # Panics
    ///
    /// Panics if `max_payload_len` is 0.
    #[must_use]
    pub fn new(max_payload_len: usize, max_memory_usage: usize) -> Self {
        Self {
            max_payload_len,
            messages: AHashMap::new(),
            bytes_left: ByteBucket::new(max_memory_usage),
        }
    }

    /// Gets the [`ByteBucket`] holding how many bytes of memory are left for
    /// storing messages.
    #[must_use]
    pub fn bytes_left(&self) -> &ByteBucket {
        &self.bytes_left
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
    /// # Errors
    ///
    /// * [`ReassembleError`]: if the message could not be reassembled properly.
    ///   It is safe and correct to ignore an error of this type.
    /// * [`NotEnoughBytes`]: if this value were to add the reassembled message
    ///   to its buffer, it would run out of bytes allocated to it. This is a
    ///   [fatal connection error](crate::packet).
    #[allow(clippy::missing_panics_doc)] // we don't expect to panic
    pub fn reassemble(
        &mut self,
        header: &FragmentHeader,
        payload: &[u8],
    ) -> Result<Result<Option<Bytes>, ReassembleError>, NotEnoughBytes> {
        // explicitly don't destructure, so that we copy the values
        let msg_seq = header.msg_seq;
        let num_frags = header.num_frags;
        let frag_index = header.frag_index;
        let nz_num_frags = match NonZeroU8::new(num_frags) {
            // if `num_frags = 0`, then `frag_index >= 0` is always true
            // so it's always an error
            None => return Ok(Err(ReassembleError::NoFrags)),
            // fast path to avoid writing this into the message buffer then
            // immediately reading it back out
            Some(num_frags) if num_frags.get() == 1 => {
                return Ok(Ok(Some(Bytes::from(payload.to_vec()))));
            }
            Some(num_frags) => num_frags,
        };
        if frag_index >= num_frags {
            return Ok(Err(ReassembleError::InvalidFragIndex { frag_index }));
        }

        let bytes_used = usize::from(num_frags) * self.max_payload_len;
        let buf = match self.messages.get_mut(&msg_seq) {
            Some(x) => x,
            None => {
                self.bytes_left.consume(bytes_used)?;
                let buf = MessageBuffer::new(self.max_payload_len, nz_num_frags);
                match self.messages.entry(msg_seq) {
                    Entry::Occupied(mut entry) => {
                        entry.insert(buf);
                        entry.into_mut()
                    }
                    Entry::Vacant(entry) => entry.insert(buf),
                }
            }
        };

        // mark this fragment as received
        let mut is_received = match buf
            .recv_frags
            .get_mut(usize::from(frag_index))
            .ok_or(ReassembleError::InvalidFragIndex { frag_index })
        {
            Ok(x) => x,
            Err(err) => return Ok(Err(err)),
        };
        if *is_received {
            return Ok(Err(ReassembleError::AlreadyReceived));
        }
        *is_received = true;
        drop(is_received);

        // and copy it into the payload buffer
        let is_last_frag = frag_index == num_frags - 1;
        let (start, end) = if is_last_frag {
            // resize the buffer down to fit this last payload
            let len = usize::from(num_frags - 1) * self.max_payload_len + payload.len();
            if len > buf.payload.len() {
                // can't shrink the buffer to a larger amount,
                // that makes no sense
                return Ok(Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.max_payload_len,
                }));
            }
            // note: explicitly don't mess with the capacity, to avoid reallocs
            // the caller can realloc if they want to, but we don't
            buf.payload.truncate(len);

            let frag_index = usize::from(frag_index);
            (
                frag_index * self.max_payload_len,
                frag_index * self.max_payload_len + payload.len(),
            )
        } else {
            if payload.len() != self.max_payload_len {
                return Ok(Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.max_payload_len,
                }));
            }

            let frag_id = usize::from(header.frag_index);
            (
                frag_id * self.max_payload_len,
                (frag_id + 1) * self.max_payload_len,
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
            let buf = self
                .messages
                .remove(&msg_seq)
                .expect("we just inserted a value into this map with this key");
            self.bytes_left.refill_exact(bytes_used);
            // this will reallocate, but consumers will always want it in Bytes
            // so it's fine
            Ok(Ok(Some(Bytes::from(buf.payload))))
        } else {
            // this message isn't complete yet, nothing to return
            Ok(Ok(None))
        }
    }

    /// Removes a message with the given sequence, dropping all its fragments.
    pub fn remove(&mut self, msg_seq: Seq) {
        let Some(buf) = self.messages.remove(&msg_seq) else {
            return;
        };
        self.bytes_left.refill_exact(buf.payload.capacity());
    }

    /// Drops all currently buffered messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.bytes_left.refill();
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    const PAYLOAD_LEN: usize = 2;

    fn frag() -> FragmentReceiver {
        FragmentReceiver::new(PAYLOAD_LEN, usize::MAX)
    }

    #[test]
    fn single() {
        let mut recv = frag();
        assert_eq!(0, recv.bytes_left.used());

        let header = FragmentHeader {
            msg_seq: Seq(1234),
            num_frags: 1,
            frag_index: 0,
        };
        assert_matches!(
            recv.reassemble(&header, &[1, 2]),
            Ok(Ok(Some(b))) if b == &[1, 2][..]
        );
        assert_eq!(0, recv.bytes_left.used());
    }

    #[test]
    fn two_frags() {
        let mut recv = frag();
        let header0 = FragmentHeader {
            msg_seq: Seq(1234),
            num_frags: 2,
            frag_index: 0,
        };
        let header1 = FragmentHeader {
            frag_index: 1,
            ..header0
        };

        assert_matches!(recv.reassemble(&header0, &[1, 2]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_left.used());

        assert_matches!(recv.reassemble(&header1, &[3, 4]), Ok(Ok(Some(b))) if b == &[1, 2, 3, 4][..]);
        assert_eq!(0, recv.bytes_left.used());
    }

    #[test]
    fn three_frags() {
        let mut recv = frag();
        let header0 = FragmentHeader {
            msg_seq: Seq(1234),
            num_frags: 3,
            frag_index: 0,
        };
        let header1 = FragmentHeader {
            frag_index: 1,
            ..header0
        };
        let header2 = FragmentHeader {
            frag_index: 2,
            ..header0
        };

        assert_matches!(recv.reassemble(&header0, &[1, 2]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 3, recv.bytes_left.used());

        assert_matches!(recv.reassemble(&header1, &[3, 4]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 3, recv.bytes_left.used());

        assert_matches!(recv.reassemble(&header2, &[5]), Ok(Ok(Some(b))) if b == &[1, 2, 3, 4, 5][..]);
        assert_eq!(0, recv.bytes_left.used());
    }

    #[test]
    fn out_of_order() {
        let mut recv = frag();
        let header0 = FragmentHeader {
            msg_seq: Seq(1234),
            num_frags: 3,
            frag_index: 0,
        };
        let header1 = FragmentHeader {
            frag_index: 1,
            ..header0
        };
        let header2 = FragmentHeader {
            frag_index: 2,
            ..header0
        };

        assert_matches!(recv.reassemble(&header1, &[3, 4]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 3, recv.bytes_left.used());

        assert_matches!(recv.reassemble(&header2, &[5]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 3, recv.bytes_left.used());

        assert_matches!(recv.reassemble(&header0, &[1, 2]), Ok(Ok(Some(b))) if b == &[1, 2, 3, 4, 5][..]);
        assert_eq!(0, recv.bytes_left.used());
    }

    #[test]
    fn invalid_header() {
        let mut recv = frag();
        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 0,
            frag_index: 0,
        };
        assert_matches!(
            recv.reassemble(&header, &[]),
            Ok(Err(ReassembleError::NoFrags))
        );

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 10,
            frag_index: 10,
        };
        assert_matches!(
            recv.reassemble(&header, &[]),
            Ok(Err(ReassembleError::InvalidFragIndex { frag_index: 10 }))
        );
    }

    #[test]
    fn already_received() {
        let mut recv = frag();
        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(Ok(None)));

        assert_matches!(
            recv.reassemble(&header, &[1, 2]),
            Ok(Err(ReassembleError::AlreadyReceived))
        );
    }

    #[test]
    fn remove() {
        let mut recv = frag();
        assert_eq!(0, recv.bytes_left.used());

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_left.used());

        let header = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 4, recv.bytes_left.used());

        recv.remove(Seq(0));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_left.used());
    }

    #[test]
    fn clear() {
        let mut recv = frag();
        assert_eq!(0, recv.bytes_left.used());

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_left.used());

        recv.clear();
        assert_eq!(0, recv.bytes_left.used());
    }

    #[test]
    fn not_enough_bytes() {
        let mut recv = FragmentReceiver::new(PAYLOAD_LEN, 4);
        assert_eq!(0, recv.bytes_left.used());

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(Ok(None)));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_left.used());
        assert_eq!(0, recv.bytes_left.get());

        let header = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 1,
            frag_index: 0,
        };
        // this one works because `num_frags = 1` - fast path
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(Ok(Some(b))) if b == &[1, 2][..]);

        let header = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Err(NotEnoughBytes));
    }
}
