use {
    crate::{
        msg::MAX_FRAGS,
        ty::{FragmentMarker, MessageSeq},
    },
    ahash::AHashMap,
    bitvec::array::BitArray,
    datasize::{data_size, DataSize},
    derivative::Derivative,
    octs::Bytes,
    std::{collections::hash_map::Entry, fmt, mem},
    web_time::Instant,
};

/// Handles receiving small message fragments produced by a [`MessageSplitter`]
/// and reassembling them into a single larger message.
///
/// [`MessageSplitter`]: crate::msg::MessageSplitter
#[derive(Derivative, Clone, DataSize)]
#[derivative(Debug)]
pub struct FragmentReceiver {
    max_payload_len: usize,
    #[derivative(Debug(format_with = "fmt_msgs"))]
    #[data_size(with = size_of_msgs)]
    msgs: AHashMap<MessageSeq, MessageBuf>,
}

fn fmt_msgs(
    value: &AHashMap<MessageSeq, MessageBuf>,
    fmt: &mut fmt::Formatter,
) -> Result<(), fmt::Error> {
    let mut seqs = value.iter().map(|(seq, _)| seq).collect::<Vec<_>>();
    seqs.sort_unstable();
    fmt.debug_set().entries(seqs).finish()
}

fn size_of_msgs(value: &AHashMap<MessageSeq, MessageBuf>) -> usize {
    value
        .iter()
        .map(|(_, buf)| mem::size_of_val(buf) + data_size(buf))
        .sum()
}

/// Failed to receive and reassemble a fragment in
/// [`FragmentReceiver::reassemble`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReassembleError {
    /// Already received a fragment with this index.
    #[error("already received this fragment")]
    AlreadyReceived,
    /// Already received a fragment which was marked as the last fragment.
    #[error("already received the last fragment")]
    AlreadyReceivedLast,
    /// Received the last fragment, but it has a lower (or the same) index
    /// number than one of the previous fragments that we've already received.
    #[error("last fragment has lower index than one of the previous fragments we've received")]
    InvalidLastFragment,
    /// Fragment does not have the correct payload length.
    #[error("invalid payload length of {len}, expected {expected}")]
    InvalidPayloadLength {
        /// Length of the payload received.
        len: usize,
        /// Length that the payload was expected to be.
        expected: usize,
    },
}

#[derive(Derivative, Clone, DataSize)]
struct MessageBuf {
    /// Index number of the last fragment, given the fragments we have received
    /// so far.
    ///
    /// Once we receive a fragment with [`FragmentMarker::is_last`] set, we know
    /// that it is the last fragment. Therefore we know the total number of
    /// fragments in this message.
    ///
    /// When we have not received the last fragment yet, this is [`None`].
    last_frag_index: Option<u8>,
    /// Largest index number of fragment we have received so far.
    max_frag_index: u8,
    /// Number of fragments we have already received.
    num_frags_recv: u8,
    /// Bit array tracking which fragment indices we have already received.
    ///
    /// We use a `(BitArray, Vec<u8>)` instead of a `Vec<Option<u8>>` for
    /// efficiency - this way, we can copy payloads directly into the `Vec<u8>`
    /// and quickly turn that buffer into a `Bytes` once we're ready to give it
    /// to the user.
    #[data_size(skip)]
    recv_frags: BitArray<[u8; MAX_FRAGS / u8::BITS as usize]>,
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
    #[data_size(skip)]
    last_recv_at: Instant,
}

impl MessageBuf {
    fn new(now: Instant, max_payload_len: usize, min_frag_index: u8) -> Self {
        Self {
            last_frag_index: None,
            max_frag_index: min_frag_index,
            num_frags_recv: 0,
            recv_frags: BitArray::default(),
            payload: vec![0; (usize::from(min_frag_index) + 1) * max_payload_len],
            last_recv_at: now,
        }
    }
}

impl FragmentReceiver {
    /// Creates a new [`FragmentReceiver`].
    ///
    /// `max_payload_len` defines the maximum length, in bytes, that the payload
    /// of a single fragment can be.
    ///
    /// # Panics
    ///
    /// Panics if `max_payload_len` is 0.
    #[must_use]
    pub fn new(max_payload_len: usize) -> Self {
        assert!(max_payload_len > 0);
        Self {
            max_payload_len,
            msgs: AHashMap::new(),
        }
    }

    /// Receives a fragment and attempts to reassemble a message from its data.
    ///
    /// `msg_seq` is a unique ID for the message that this fragment is a part
    /// of.
    ///
    /// If this returns `Ok(None)`, then we know that this message has more than
    /// one fragment in it, and we are awaiting more fragments.
    ///
    /// If this returns `Ok(Some(..))`, then we have fully reassembled this
    /// message, and the caller gets the reassembled [`Bytes`] message.
    ///
    /// # Errors
    ///
    /// Errors if the fragment was not valid for reassembly.
    ///
    /// Since transmission errors occur such as duplicated and dropped packets,
    /// it is perfectly safe to ignore these errors.
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn reassemble(
        &mut self,
        now: Instant,
        msg_seq: MessageSeq,
        marker: FragmentMarker,
        payload: impl AsRef<[u8]>,
    ) -> Result<Option<Bytes>, ReassembleError> {
        let payload = payload.as_ref();
        let frag_index = marker.index();
        let frag_index_u = usize::from(frag_index);

        // create buffer for this message if it doesn't exist yet
        let buf = match self.msgs.entry(msg_seq) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                entry.insert(MessageBuf::new(now, self.max_payload_len, frag_index))
            }
        };

        // check if this fragment has been received yet
        if buf.recv_frags[frag_index_u] {
            return Err(ReassembleError::AlreadyReceived);
        }

        // copy the payload data into the buffer
        let start = frag_index_u * self.max_payload_len;
        let end = start + payload.len();
        if marker.is_last() {
            if buf.last_frag_index.is_some() {
                return Err(ReassembleError::AlreadyReceivedLast);
            }
            if frag_index < buf.max_frag_index {
                return Err(ReassembleError::InvalidLastFragment);
            }

            buf.last_frag_index = Some(frag_index);
            buf.payload.resize(end, 0);
        } else {
            if payload.len() != self.max_payload_len {
                return Err(ReassembleError::InvalidPayloadLength {
                    len: payload.len(),
                    expected: self.max_payload_len,
                });
            }

            if end >= buf.payload.len() {
                buf.payload.resize(end, 0);
            }
        }
        buf.payload[start..end].copy_from_slice(payload);

        // only update the buffer meta once we know there are no more error paths
        buf.recv_frags.set(frag_index_u, true);
        buf.max_frag_index = buf.max_frag_index.max(frag_index);
        buf.last_recv_at = now;

        // if we've fully reassembled the message, we can return it now
        if buf
            .last_frag_index
            .is_some_and(|last| buf.num_frags_recv >= last)
        {
            let buf = self.msgs.remove(&msg_seq).expect(
                "we already have a mut ref to the buffer at this key, so we should be able to \
                 remove and take ownership of it",
            );
            Ok(Some(Bytes::from(buf.payload)))
        } else {
            // this happens separately from the other buffer meta update
            // so that the `if` condition above works properly
            buf.num_frags_recv += 1;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::msg::MAX_FRAG_INDEX};

    const SEQ: MessageSeq = MessageSeq::ZERO;
    const SEQ1: MessageSeq = MessageSeq::new(1);
    const SEQ2: MessageSeq = MessageSeq::new(2);

    fn non_last(index: u8) -> FragmentMarker {
        FragmentMarker::non_last(index).unwrap()
    }

    fn last(index: u8) -> FragmentMarker {
        FragmentMarker::last(index).unwrap()
    }

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    #[should_panic]
    fn zero_payload_len() {
        _ = FragmentReceiver::new(0);
    }

    #[test]
    fn empty() {
        let mut r = FragmentReceiver::new(1);
        assert_eq!(0, data_size(&r));

        assert_eq!(
            Bytes::from(vec![]),
            r.reassemble(now(), SEQ, last(0), []).unwrap().unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn half_frag() {
        let mut r = FragmentReceiver::new(2);
        assert_eq!(
            Bytes::from(vec![1]),
            r.reassemble(now(), SEQ, last(0), [1]).unwrap().unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn one_frag() {
        let mut r = FragmentReceiver::new(2);
        assert_eq!(
            Bytes::from(vec![1, 2]),
            r.reassemble(now(), SEQ, last(0), [1, 2]).unwrap().unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn one_half_frags() {
        let mut r = FragmentReceiver::new(2);
        assert!(r.reassemble(now(), SEQ, last(1), [3]).unwrap().is_none());
        assert!(data_size(&r) > 0);
        assert_eq!(
            Bytes::from(vec![1, 2, 3]),
            r.reassemble(now(), SEQ, non_last(0), [1, 2])
                .unwrap()
                .unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn one_half_frags_opposite_order() {
        let mut r = FragmentReceiver::new(2);
        assert!(r
            .reassemble(now(), SEQ, non_last(0), [1, 2])
            .unwrap()
            .is_none());
        assert!(data_size(&r) > 0);
        assert_eq!(
            Bytes::from(vec![1, 2, 3]),
            r.reassemble(now(), SEQ, last(1), [3]).unwrap().unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn two_frags() {
        let mut r = FragmentReceiver::new(2);
        assert!(r.reassemble(now(), SEQ, last(1), [3, 4]).unwrap().is_none());
        assert!(data_size(&r) > 0);
        assert_eq!(
            Bytes::from(vec![1, 2, 3, 4]),
            r.reassemble(now(), SEQ, non_last(0), [1, 2])
                .unwrap()
                .unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn already_received() {
        let mut r = FragmentReceiver::new(2);
        assert!(r
            .reassemble(now(), SEQ, non_last(0), [1, 2])
            .unwrap()
            .is_none());
        assert_eq!(
            ReassembleError::AlreadyReceived,
            r.reassemble(now(), SEQ, non_last(0), [1, 2]).unwrap_err()
        );
        assert_eq!(
            ReassembleError::AlreadyReceived,
            // different payload
            r.reassemble(now(), SEQ, non_last(0), [3, 4]).unwrap_err()
        );
    }

    #[test]
    fn two_last_frags() {
        let mut r = FragmentReceiver::new(2);
        assert!(r.reassemble(now(), SEQ, last(1), [1]).unwrap().is_none());
        assert_eq!(
            ReassembleError::AlreadyReceivedLast,
            r.reassemble(now(), SEQ, last(2), [1]).unwrap_err()
        );
    }

    #[test]
    fn invalid_last_frag() {
        let mut r = FragmentReceiver::new(2);
        assert!(r
            .reassemble(now(), SEQ, non_last(1), [1, 2])
            .unwrap()
            .is_none());
        assert_eq!(
            ReassembleError::InvalidLastFragment,
            r.reassemble(now(), SEQ, last(0), []).unwrap_err()
        );
    }

    #[test]
    fn lower_index_last_frag() {
        let mut r = FragmentReceiver::new(2);
        assert!(r
            .reassemble(now(), SEQ, non_last(10), [1, 2])
            .unwrap()
            .is_none());
        assert_eq!(
            ReassembleError::InvalidLastFragment,
            r.reassemble(now(), SEQ, last(0), [3]).unwrap_err()
        );
    }

    #[test]
    fn invalid_payload_len() {
        let mut r = FragmentReceiver::new(2);
        assert_eq!(
            ReassembleError::InvalidPayloadLength {
                len: 0,
                expected: 2
            },
            r.reassemble(now(), SEQ, non_last(0), []).unwrap_err()
        );
        assert_eq!(
            ReassembleError::InvalidPayloadLength {
                len: 1,
                expected: 2
            },
            r.reassemble(now(), SEQ, non_last(1), [1]).unwrap_err()
        );
    }

    #[test]
    fn max_frags() {
        let mut r = FragmentReceiver::new(2);
        for index in 0..=(MAX_FRAG_INDEX - 1) {
            assert!(r
                .reassemble(now(), SEQ, non_last(index), [1, 1])
                .unwrap()
                .is_none());
        }
        assert_eq!(
            Bytes::from(vec![1; MAX_FRAGS * 2]),
            r.reassemble(now(), SEQ, last(MAX_FRAG_INDEX), [1, 1])
                .unwrap()
                .unwrap()
        );
    }

    #[test]
    fn two_msgs_one_frag() {
        let mut r = FragmentReceiver::new(2);
        assert_eq!(
            Bytes::from(vec![1, 2]),
            r.reassemble(now(), SEQ1, last(0), [1, 2]).unwrap().unwrap()
        );
        assert_eq!(
            Bytes::from(vec![3, 4]),
            r.reassemble(now(), SEQ2, last(0), [3, 4]).unwrap().unwrap()
        );
        assert_eq!(0, data_size(&r));
    }

    #[test]
    fn two_msgs_two_frags() {
        let mut r = FragmentReceiver::new(2);
        assert!(r
            .reassemble(now(), SEQ1, non_last(0), [1, 2])
            .unwrap()
            .is_none());
        assert!(r
            .reassemble(now(), SEQ2, non_last(0), [4, 5])
            .unwrap()
            .is_none());
        let data_size_1 = data_size(&r);
        assert!(data_size_1 > 0);

        assert_eq!(
            Bytes::from(vec![1, 2, 3]),
            r.reassemble(now(), SEQ1, last(1), [3]).unwrap().unwrap()
        );
        let data_size_2 = data_size(&r);
        assert!(data_size_2 < data_size_1);
        assert!(data_size_2 > 0);

        assert_eq!(
            Bytes::from(vec![4, 5, 6]),
            r.reassemble(now(), SEQ2, last(1), [6]).unwrap().unwrap()
        );
        assert_eq!(0, data_size(&r));
    }
}
