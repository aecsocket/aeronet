use std::{collections::hash_map::Entry, num::NonZeroU8};

use ahash::AHashMap;
use bitvec::{array::BitArray, bitarr};
use octs::Bytes;
use web_time::Instant;

use crate::seq::Seq;

use super::FragmentHeader;

/// Handles reassembling small message fragments into one larger message which
/// was fragmented by a [`FragmentSender`].
///
/// See [`frag`](crate::frag).
///
/// # Memory management
///
/// Care must be taken to ensure that a malicious peer can't cause a
/// denial-of-service by having our side buffer too many fragments. See
/// [`frag`](crate::frag) to learn about how to manage this.
///
/// [`FragmentSender`]: crate::frag::FragmentSender
#[derive(Debug, Clone)]
pub struct FragmentReceiver {
    max_payload_len: usize,
    messages: AHashMap<Seq, MessageBuffer>,
    bytes_used: usize,
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
    #[error("invalid fragment index {frag_index}")]
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
    ///
    /// # Panics
    ///
    /// Panics if `max_payload_len` is 0.
    #[must_use]
    pub fn new(max_payload_len: usize) -> Self {
        assert!(max_payload_len > 0);
        Self {
            max_payload_len,
            messages: AHashMap::new(),
            bytes_used: 0,
        }
    }

    /// Gets the total number of bytes used for storing messages which have not
    /// been fully reassembled yet.
    #[must_use]
    pub fn bytes_used(&self) -> usize {
        self.bytes_used
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
    /// Errors if the message could not be reassembled properly. It is safe and
    /// correct to ignore an error of this type.
    #[allow(clippy::missing_panics_doc)] // we don't expect to panic
    pub fn reassemble(
        &mut self,
        header: &FragmentHeader,
        payload: impl AsRef<[u8]>,
    ) -> Result<Option<Bytes>, ReassembleError> {
        let payload = payload.as_ref();
        // explicitly don't destructure, so that we copy the values
        let msg_seq = header.msg_seq;
        let num_frags = header.num_frags;
        let frag_index = header.frag_index;
        let nz_num_frags = match NonZeroU8::new(num_frags) {
            // if `num_frags = 0`, then `frag_index >= 0` is always true
            // so it's always an error
            None => return Err(ReassembleError::NoFrags),
            // fast path to avoid writing this into the message buffer then
            // immediately reading it back out
            Some(num_frags) if num_frags.get() == 1 => {
                return Ok(Some(Bytes::from(payload.to_vec())));
            }
            Some(num_frags) => num_frags,
        };
        if frag_index >= num_frags {
            return Err(ReassembleError::InvalidFragIndex { frag_index });
        }

        let buf = match self.messages.get_mut(&msg_seq) {
            Some(buf) => buf,
            None => {
                let buf = MessageBuffer::new(self.max_payload_len, nz_num_frags);
                // [1] when we initially create the message buffer, we assume that the
                // last fragment has the same length as all the other fragments
                self.bytes_used += buf.payload.capacity();
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
        let mut is_received = buf
            .recv_frags
            .get_mut(usize::from(frag_index))
            .ok_or(ReassembleError::InvalidFragIndex { frag_index })?;
        if *is_received {
            return Err(ReassembleError::AlreadyReceived);
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
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.max_payload_len,
                });
            }
            buf.payload.truncate(len);
            // [2] although we've truncated the payload buffer, we keep the allocation,
            // so we don't change `bytes_used`

            let frag_index = usize::from(frag_index);
            (
                frag_index * self.max_payload_len,
                frag_index * self.max_payload_len + payload.len(),
            )
        } else {
            if payload.len() != self.max_payload_len {
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.max_payload_len,
                });
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
            // [3] now we've given away the allocation, we can reclaim the bytes used
            self.bytes_used -= buf.payload.capacity();
            // this will reallocate, but consumers will always want it in Bytes
            // so it's fine
            Ok(Some(Bytes::from(buf.payload)))
        } else {
            // this message isn't complete yet, nothing to return
            Ok(None)
        }
    }

    /// Removes a message with the given sequence, dropping all its fragments.
    pub fn remove(&mut self, msg_seq: Seq) {
        let Some(buf) = self.messages.remove(&msg_seq) else {
            return;
        };
        self.bytes_used -= buf.payload.capacity();
    }

    /// Drops all currently buffered messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.bytes_used = 0;
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    const PAYLOAD_LEN: usize = 2;

    fn recv() -> FragmentReceiver {
        FragmentReceiver::new(PAYLOAD_LEN)
    }

    #[test]
    fn single() {
        const SEQ: Seq = Seq(1234);

        let mut recv = recv();
        assert_eq!(0, recv.bytes_used());

        let header = FragmentHeader {
            msg_seq: SEQ,
            num_frags: 1,
            frag_index: 0,
        };
        assert_matches!(
            recv.reassemble(&header, &[1, 2]),
            Ok(Some(b)) if b == &[1, 2][..]
        );
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn two_frags() {
        const SEQ: Seq = Seq(1234);
        const NUM_FRAGS: usize = 2;

        let mut recv = recv();
        let data: [_; NUM_FRAGS] = [
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 2,
                    frag_index: 0,
                },
                [1, 2].as_slice(),
            ),
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 2,
                    frag_index: 1,
                },
                [3, 4].as_slice(),
            ),
        ];

        assert_matches!(recv.reassemble(&data[0].0, data[0].1), Ok(None));
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(recv.reassemble(&data[1].0, data[1].1), Ok(Some(b)) if b == &[1, 2, 3, 4][..]);
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn three_frags() {
        const SEQ: Seq = Seq(1234);
        const NUM_FRAGS: usize = 3;

        let mut recv = recv();
        let data: [_; NUM_FRAGS] = [
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 3,
                    frag_index: 0,
                },
                [1, 2].as_slice(),
            ),
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 3,
                    frag_index: 1,
                },
                [3, 4].as_slice(),
            ),
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 3,
                    frag_index: 2,
                },
                [5].as_slice(),
            ),
        ];

        assert_matches!(recv.reassemble(&data[0].0, data[0].1), Ok(None));
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(recv.reassemble(&data[1].0, data[1].1), Ok(None));
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(recv.reassemble(&data[2].0, data[2].1), Ok(Some(b)) if b == &[1, 2, 3, 4, 5][..]);
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn out_of_order() {
        const SEQ: Seq = Seq(1234);
        const NUM_FRAGS: usize = 3;

        let mut recv = recv();
        let data: [_; NUM_FRAGS] = [
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 3,
                    frag_index: 0,
                },
                [1, 2].as_slice(),
            ),
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 3,
                    frag_index: 1,
                },
                [3, 4].as_slice(),
            ),
            (
                FragmentHeader {
                    msg_seq: SEQ,
                    num_frags: 3,
                    frag_index: 2,
                },
                [5].as_slice(),
            ),
        ];

        assert_matches!(recv.reassemble(&data[0].0, data[0].1), Ok(None));
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(recv.reassemble(&data[1].0, data[1].1), Ok(None));
        // notice here, that even though we've just reassembled the last
        // fragment, we haven't deallocated any data
        // so we're still using the same amount of memory internally
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(recv.reassemble(&data[2].0, data[2].1), Ok(Some(b)) if b == &[1, 2, 3, 4, 5][..]);
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn invalid_header() {
        let mut recv = recv();
        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 0,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[]), Err(ReassembleError::NoFrags));

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 10,
            frag_index: 10,
        };
        assert_matches!(
            recv.reassemble(&header, &[]),
            Err(ReassembleError::InvalidFragIndex { frag_index: 10 })
        );
    }

    #[test]
    fn already_received() {
        let mut recv = recv();
        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(None));

        assert_matches!(
            recv.reassemble(&header, &[1, 2]),
            Err(ReassembleError::AlreadyReceived)
        );
    }

    #[test]
    fn remove() {
        let mut recv = recv();
        assert_eq!(0, recv.bytes_used());

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(None));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_used());

        let header = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(None));
        assert_eq!(PAYLOAD_LEN * 2 * 2, recv.bytes_used());

        recv.remove(Seq(0));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_used());
    }

    #[test]
    fn clear() {
        let mut recv = recv();
        assert_eq!(0, recv.bytes_used());

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 2,
            frag_index: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(None));
        assert_eq!(2 * 2, recv.bytes_used());

        recv.clear();
        assert_eq!(0, recv.bytes_used());
    }
}
