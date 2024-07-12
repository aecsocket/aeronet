use std::{cmp::Ordering, collections::hash_map::Entry, num::NonZeroU8};

use ahash::AHashMap;
use bitvec::{array::BitArray, bitarr};
use octs::Bytes;
use web_time::Instant;

use crate::{packet::MessageSeq, seq::Seq};

use super::FragmentHeader;

/// Handles reassembling small message fragments, produced by a
/// [`FragmentSender`], into one larger message.
///
/// See [`frag`](crate::frag).
///
/// # Memory management
///
/// Care must be taken to ensure that a malicious peer can't cause a
/// denial-of-service by having the receiver buffer too many fragments. See
/// [`frag`](crate::frag) to learn about how to manage this.
///
/// [`FragmentSender`]: crate::frag::FragmentSender
#[derive(Debug, Clone)]
pub struct FragmentReceiver {
    max_payload_len: usize,
    messages: AHashMap<MessageSeq, MessageBuffer>,
    bytes_used: usize,
}

#[derive(Debug, Clone)]
struct MessageBuffer {
    /// Index number of the last fragment, given the fragments we have received
    /// so far.
    ///
    /// The last fragment has a bit set in its marker. Once we receive that
    /// fragment, we know that it is the last one, therefore we know in total
    /// how many fragments there are in this message.
    ///
    /// When we have not received the last fragment yet, this is [`None`].
    last_frag_index: Option<u8>,
    /// Number of fragments we have already received.
    num_frags_recv: u8,
    /// Bit array tracking which fragment indices we have already received.
    ///
    /// We use a `(BitArray, Vec<u8>)` instead of a `Vec<Option<u8>>` for
    /// efficiency - this way, we can copy payloads directly into the `Vec<u8>`
    /// and quickly turn that buffer into a `Bytes` once we're ready to give it
    /// to the user.
    recv_frags: BitArray<[u8; 32]>,
    /// Combination of all fragment payloads reassembled into a single buffer.
    ///
    /// Initially, we allocate enough capacity for however many fragments we
    /// know there are at a minimum. For example, if the first fragment we
    /// receive has an index of 4, then we know there's at least 5 total
    /// fragments in this message. If we later receive a fragment for the same
    /// message with index 6, we know there's at least 7 total fragments, so we
    /// resize the buffer and increase the capacity.
    payload: Vec<u8>,
    /// Last instant at which we received a new fragment for this message.
    last_recv_at: Instant,
}

/// Error that occurs when using [`FragmentReceiver::reassemble`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReassembleError {
    /// The fragment for the given index was already received.
    #[error("already received this fragment")]
    AlreadyReceived,
    #[error("this cannot be the last fragment")]
    InvalidLastFragment,
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
    fn new(now: Instant, max_payload_len: usize, min_frag_index: u8) -> Self {
        Self {
            last_frag_index: None,
            num_frags_recv: 0,
            recv_frags: bitarr![u8, bitvec::order::Lsb0; 0; 256],
            payload: vec![0; (usize::from(min_frag_index) + 1) * max_payload_len],
            last_recv_at: now,
        }
    }
}

impl FragmentReceiver {
    /// Creates a new [`FragmentReceiver`].
    ///
    /// `max_payload_len` defines the maximum length, in bytes, that the
    /// payload of a single fragmented packet can be. This must be greater
    /// than 0.
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
        now: Instant,
        header: &FragmentHeader,
        payload: impl AsRef<[u8]>,
    ) -> Result<Option<Bytes>, ReassembleError> {
        let payload = payload.as_ref();
        // explicitly don't destructure, so that we copy the values instead of
        // taking a ref
        let msg_seq = header.msg_seq;
        let marker = header.marker;
        let frag_index = marker.index();

        // create buffer for this message if it doesn't exist yet
        let buf = match self.messages.entry(msg_seq) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let buf = MessageBuffer::new(now, self.max_payload_len, frag_index);
                self.bytes_used += buf.payload.capacity();
                entry.insert(buf)
            }
        };

        // check if this fragment has been received yet
        let mut is_received = buf
            .recv_frags
            .get_mut(usize::from(frag_index))
            .expect("`recv_frags` has 256 bits, and we are indexing using a `u8`");
        if *is_received {
            return Err(ReassembleError::AlreadyReceived);
        }

        // copy the payload data into the buffer
        let len_to_copy = if marker.is_last() {
            if buf.last_frag_index.is_some() {
                return Err(ReassembleError::AlreadyReceived);
            }
            buf.last_frag_index = Some(frag_index);

            if buf.num_frags_recv > frag_index {
                // we've received more fragments than there are supposed to be
                // so our peer sent us a bad fragment
                return Err(ReassembleError::InvalidLastFragment);
            }

            let new_len = usize::from(frag_index) * self.max_payload_len + payload.len();
            // TODO
            buf.payload.resize(new_len, 0);
            // END TODO

            payload.len()
        } else {
            if payload.len() != self.max_payload_len {
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.max_payload_len,
                });
            }

            self.max_payload_len
        };
        let start = usize::from(frag_index) * self.max_payload_len;
        let end = start + len_to_copy;
        // TODO
        if end > buf.payload.len() {
            self.bytes_used += end - buf.payload.len();
            buf.payload.resize(end, 0);
        }
        // END TODO
        buf.payload[start..end].copy_from_slice(payload);

        // only update the buffer meta once we know there are no more error paths
        buf.num_frags_recv += 1;
        buf.last_recv_at = now;
        *is_received = true;
        drop(is_received);

        // if we've fully reassembled the message, we can return it now
        if buf
            .last_frag_index
            .is_some_and(|last| buf.num_frags_recv >= last)
        {
            let buf = self
                .messages
                .remove(&msg_seq)
                .expect("`buf` is a mut ref to this buffer");
            debug_assert_eq!(buf.payload.capacity(), buf.payload.len());
            self.bytes_used -= buf.payload.capacity();
            Ok(Some(Bytes::from(buf.payload)))
        } else {
            Ok(None)
        }
    }

    /// Removes a message with the given sequence, dropping all its fragments.
    pub fn remove(&mut self, msg_seq: MessageSeq) {
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

    use crate::frag::{Fragment, FragmentMarker};

    use super::*;

    const PAYLOAD_LEN: usize = 2;

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn single() {
        const MSG_SEQ: MessageSeq = MessageSeq::new(1234);

        let mut recv = FragmentReceiver::new(PAYLOAD_LEN);
        assert_eq!(0, recv.bytes_used());

        let frag = Fragment {
            header: FragmentHeader {
                msg_seq: MSG_SEQ,
                marker: FragmentMarker::last(0).unwrap(),
            },
            payload: vec![1, 2].into(),
        };
        assert_matches!(
            recv.reassemble(now(), &frag.header, frag.payload),
            Ok(Some(b)) if b == &[1, 2][..]
        );
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn two_frags_in_order() {
        const MSG_SEQ: MessageSeq = MessageSeq::new(1234);
        const NUM_FRAGS: usize = 2;

        let mut recv = FragmentReceiver::new(PAYLOAD_LEN);
        let frags: [_; NUM_FRAGS] = [
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(0).unwrap(),
                },
                payload: vec![1, 2].into(),
            },
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(1).unwrap(),
                },
                payload: vec![3, 4].into(),
            },
        ];

        assert_matches!(
            recv.reassemble(now(), &frags[0].header, &frags[0].payload),
            Ok(None)
        );
        // receiver knows it'll take at least 1 fragment right now, but it
        // doesn't know about the 2nd one
        assert_eq!(PAYLOAD_LEN * 1, recv.bytes_used());

        assert_matches!(recv.reassemble(now(), &frags[1].header, &frags[1].payload), Ok(Some(b)) if b == &[1, 2, 3, 4][..]);
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn two_frags_out_of_order() {
        const MSG_SEQ: MessageSeq = MessageSeq::new(1234);

        let mut recv = FragmentReceiver::new(PAYLOAD_LEN);
        let frags = [
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(0).unwrap(),
                },
                payload: vec![1, 2].into(),
            },
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(1).unwrap(),
                },
                payload: vec![3, 4].into(),
            },
        ];

        assert_matches!(
            recv.reassemble(now(), &frags[1].header, &frags[1].payload),
            Ok(None)
        );
        // receiver knows that the 2nd fragment is the last one, so it knows
        // the total size
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_used());

        assert_matches!(recv.reassemble(now(), &frags[0].header, &frags[0].payload), Ok(Some(b)) if b == &[1, 2, 3, 4][..]);
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn three_frags() {
        const MSG_SEQ: MessageSeq = MessageSeq::new(1234);
        const NUM_FRAGS: usize = 3;

        let mut recv = FragmentReceiver::new(PAYLOAD_LEN);
        let frags: [_; NUM_FRAGS] = [
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(0).unwrap(),
                },
                payload: vec![1, 2].into(),
            },
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(1).unwrap(),
                },
                payload: vec![3, 4].into(),
            },
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(2).unwrap(),
                },
                payload: vec![5].into(),
            },
        ];

        assert_matches!(
            recv.reassemble(now(), &frags[0].header, &frags[0].payload),
            Ok(None)
        );
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(
            recv.reassemble(now(), &frags[1].header, &frags[1].payload),
            Ok(None)
        );
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(
            recv.reassemble(now(), &frags[2].header, &frags[2].payload),
            Ok(Some(b)) if b == &[1, 2, 3, 4, 5][..]
        );
        assert_eq!(0, recv.bytes_used());
    }

    #[test]
    fn out_of_order() {
        const MSG_SEQ: MessageSeq = MessageSeq::new(1234);
        const NUM_FRAGS: usize = 3;

        let mut recv = FragmentReceiver::new(PAYLOAD_LEN);
        let frags: [_; NUM_FRAGS] = [
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(0).unwrap(),
                },
                payload: vec![1, 2].into(),
            },
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(1).unwrap(),
                },
                payload: vec![3, 4].into(),
            },
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(2).unwrap(),
                },
                payload: vec![5].into(),
            },
        ];

        assert_matches!(
            recv.reassemble(now(), &frags[0].header, &frags[0].payload),
            Ok(None)
        );
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(
            recv.reassemble(now(), &frags[1].header, &frags[1].payload),
            Ok(None)
        );
        // notice here, that even though we've just reassembled the last
        // fragment, we haven't deallocated any data
        // so we're still using the same amount of memory internally
        assert_eq!(PAYLOAD_LEN * NUM_FRAGS, recv.bytes_used());

        assert_matches!(
            recv.reassemble(now(), &frags[2].header, &frags[2].payload),
            Ok(Some(b)) if b == &[1, 2, 3, 4, 5][..]
        );
        assert_eq!(0, recv.bytes_used());
    }

    /*
    #[test]
    fn invalid_header() {
        let mut recv = recv();
        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 0,
            marker: 0,
        };
        assert_matches!(recv.reassemble(&header, &[]), Err(ReassembleError::NoFrags));

        let header = FragmentHeader {
            msg_seq: Seq(0),
            num_frags: 10,
            marker: 10,
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
            marker: 0,
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
            marker: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(None));
        assert_eq!(PAYLOAD_LEN * 2, recv.bytes_used());

        let header = FragmentHeader {
            msg_seq: Seq(1),
            num_frags: 2,
            marker: 0,
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
            marker: 0,
        };
        assert_matches!(recv.reassemble(&header, &[1, 2]), Ok(None));
        assert_eq!(2 * 2, recv.bytes_used());

        recv.clear();
        assert_eq!(0, recv.bytes_used());
    }*/
}
