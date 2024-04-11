use std::{
    num::NonZeroU8,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use bitvec::{array::BitArray, bitarr};
use bytes::Bytes;

use crate::seq::Seq;

use super::FragmentHeader;

#[derive(Debug, Clone)]
pub struct FragmentReceiver {
    max_payload_len: usize,
    messages: AHashMap<Seq, MessageBuffer>,
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
    ///
    /// # Panics
    ///
    /// Panics if `max_payload_len` is 0.
    pub fn new(max_payload_len: usize) -> Self {
        Self {
            max_payload_len,
            messages: AHashMap::new(),
        }
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
    /// Errors if the message could not be reassembled properly.
    ///
    /// It is perfectly safe to ignore these errors - they are provided more
    /// for clarity on why reassembly failed, rather than a fatal error
    /// condition for a connection.
    pub fn reassemble(
        &mut self,
        header: &FragmentHeader,
        payload: &[u8],
    ) -> Result<Option<Bytes>, ReassembleError> {
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

        let buf = self
            .messages
            .entry(msg_seq)
            .or_insert_with(|| MessageBuffer::new(self.max_payload_len, nz_num_frags));

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

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    #[test]
    fn single() {
        let mut recv = FragmentReceiver::new(2);
        assert_eq!(
            &[1, 2,][..],
            recv.reassemble(
                &FragmentHeader {
                    msg_seq: Seq(1234),
                    num_frags: 1,
                    frag_index: 0
                },
                &[1, 2,]
            )
            .unwrap()
            .unwrap()
        );
    }

    #[test]
    fn multiple() {
        let mut recv = FragmentReceiver::new(2);
        let header = FragmentHeader {
            msg_seq: Seq(1234),
            num_frags: 3,
            frag_index: 0,
        };
        assert_eq!(
            None,
            recv.reassemble(
                &FragmentHeader {
                    frag_index: 0,
                    ..header
                },
                &[1, 2]
            )
            .unwrap()
        );
        assert_eq!(
            None,
            recv.reassemble(
                &FragmentHeader {
                    frag_index: 1,
                    ..header
                },
                &[3, 4]
            )
            .unwrap()
        );
        assert_eq!(
            &[1, 2, 3, 4, 5][..],
            recv.reassemble(
                &FragmentHeader {
                    frag_index: 2,
                    ..header
                },
                &[5]
            )
            .unwrap()
            .unwrap()
        );
    }

    #[test]
    fn out_of_order() {
        let mut recv = FragmentReceiver::new(2);
        let header = FragmentHeader {
            msg_seq: Seq(1234),
            num_frags: 2,
            frag_index: 0,
        };
        assert_eq!(
            None,
            recv.reassemble(
                &FragmentHeader {
                    frag_index: 1,
                    ..header
                },
                &[3]
            )
            .unwrap()
        );
        assert_eq!(
            &[1, 2, 3][..],
            recv.reassemble(
                &FragmentHeader {
                    frag_index: 0,
                    ..header
                },
                &[1, 2]
            )
            .unwrap()
            .unwrap()
        );
    }

    #[test]
    fn invalid_header() {
        let mut recv = FragmentReceiver::new(1024);
        assert_matches!(
            recv.reassemble(
                &FragmentHeader {
                    msg_seq: Seq(0),
                    num_frags: 0,
                    frag_index: 0,
                },
                &[],
            )
            .unwrap_err(),
            ReassembleError::NoFrags
        );
        assert_matches!(
            recv.reassemble(
                &FragmentHeader {
                    msg_seq: Seq(0),
                    num_frags: 10,
                    frag_index: 10,
                },
                &[]
            )
            .unwrap_err(),
            ReassembleError::InvalidFragIndex { frag_index: 10 }
        );
    }
}
