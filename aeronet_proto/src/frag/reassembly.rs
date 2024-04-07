use std::{
    num::NonZeroU8,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use bitvec::{array::BitArray, bitarr};

use crate::seq::Seq;

use super::FragmentHeader;

#[derive(Debug, Clone)]
pub struct Reassembly {
    payload_len: usize,
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
    #[error("invalid payload length - length: {len}, expected: {expected}")]
    InvalidPayloadLength {
        /// Length of the payload received.
        len: usize,
        /// Exact length that the payload was expected to be.
        expected: usize,
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

impl MessageBuffer {
    fn new(payload_len: usize, header: &FragmentHeader, num_frags: NonZeroU8) -> Self {
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
            payload: vec![0; usize::from(header.num_frags) * payload_len],
            last_recv_at: Instant::now(),
        }
    }
}

impl Reassembly {
    /// Creates a new [`Reassembly`].
    ///
    /// * `payload_len` defines the maximum length, in bytes, that the payload
    ///   of a single fragmented packet can be. This must be greater than 0.
    ///
    /// # Panics
    ///
    /// Panics if `payload_len` is 0.
    pub fn new(payload_len: usize) -> Self {
        Self {
            payload_len,
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
        header: &FragmentHeader,
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

        let mut buf = self
            .messages
            .remove(&header.msg_seq)
            .unwrap_or_else(|| MessageBuffer::new(self.payload_len, header, num_frags));

        // mark this fragment as received
        let frag_id = usize::from(header.frag_id);
        let mut is_received =
            buf.recv_frags
                .get_mut(frag_id)
                .ok_or(ReassembleError::InvalidFragId {
                    frag_id: header.frag_id,
                })?;
        if *is_received {
            drop(is_received);
            self.messages.insert(header.msg_seq, buf);
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
                self.messages.insert(header.msg_seq, buf);
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
                self.messages.insert(header.msg_seq, buf);
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.payload_len,
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
            Ok(Some(buf.payload))
        } else {
            // this message isn't complete yet, nothing to return
            // add it (back) to the messages
            self.messages.insert(header.msg_seq, buf);
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
    fn invalid_header() {
        let mut asm = Reassembly::new(1024);
        assert_eq!(
            None,
            asm.reassemble(
                &FragmentHeader {
                    msg_seq: Seq(0),
                    num_frags: 0,
                    frag_id: 0,
                },
                &[],
            )
            .unwrap()
        );
        assert_matches!(
            asm.reassemble(
                &FragmentHeader {
                    msg_seq: Seq(0),
                    num_frags: 10,
                    frag_id: 10,
                },
                &[]
            )
            .unwrap_err(),
            ReassembleError::InvalidFragId { frag_id: 1 }
        );
    }
}
