use std::iter::FusedIterator;

use octs::{chunks::ByteChunksExt, Buf, Bytes};

use crate::{frag::MAX_FRAGS, packet::MessageSeq};

use super::{Fragment, FragmentHeader, FragmentMarker};

/// Handles splitting a single large message into multiple smaller fragments
/// which can be reassembled by a [`FragmentReceiver`].
///
/// See [`frag`](crate::frag).
///
/// [`FragmentReceiver`]: crate::frag::FragmentReceiver
#[derive(Debug, Clone)]
pub struct FragmentSender {
    max_payload_len: usize,
}

/// Attempted to fragment a message which was too big using
/// [`FragmentSender::fragment`].
#[derive(Debug, Clone, thiserror::Error)]
#[error("message too big - {len} / {max} bytes")]
pub struct MessageTooBig {
    /// Length of the message in bytes.
    len: usize,
    /// Maximum length of the message in bytes.
    max: usize,
}

impl FragmentSender {
    /// Creates a new [`FragmentSender`].
    ///
    /// `max_payload_len` defines the maximum length, in bytes, that the payload
    /// of a single fragmented packet can be. This must be greater than 0.
    ///
    /// # Panics
    ///
    /// Panics if `max_payload_len` is 0.
    #[must_use]
    pub fn new(max_payload_len: usize) -> Self {
        assert!(max_payload_len > 0);
        Self { max_payload_len }
    }

    /// Gets the maximum payload length as defined on construction.
    #[must_use]
    pub const fn max_payload_len(&self) -> usize {
        self.max_payload_len
    }

    /// Splits a message up into individual fragments and combines them with
    /// per-fragment metadata, ready to be reassembled by a
    /// [`FragmentReceiver`].
    ///
    /// `msg_seq` represents the sequence number of this specific message (note
    /// that this is different from the *packet* sequence number - fragments may
    /// be sent out in different packets, with different packet sequence
    /// numbers).
    ///
    /// Fragments are returned in the opposite order to the fragment index -
    /// that is, if a message is split into fragments A, B, C, the iterator will
    /// return them in the order C, B, A. This is done to make reassembly more
    /// efficient, since when the receiver receives C, it will immediately know
    /// how many fragments there are total, and can allocate the right sized
    /// buffer to fit this message.
    ///
    /// If `msg` is empty, this will return an empty iterator.
    ///
    /// # Errors
    ///
    /// Errors if the message is larger than `MAX_FRAGS * max_payload_len`.
    ///
    /// [`FragmentReceiver`]: crate::frag::FragmentReceiver
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment(
        &self,
        msg_seq: MessageSeq,
        msg: Bytes,
    ) -> Result<impl ExactSizeIterator<Item = Fragment> + FusedIterator, MessageTooBig> {
        let max_frags = usize::from(MAX_FRAGS);
        let msg_len = msg.remaining();
        let iter = msg.byte_chunks(self.max_payload_len).enumerate().rev();
        u8::try_from(iter.len()).map_err(|_| MessageTooBig {
            len: msg_len,
            max: max_frags * self.max_payload_len,
        })?;
        debug_assert!(iter.len() <= max_frags);

        let last_index = iter.len() - 1;
        Ok(iter.map(move |(index, payload)| {
            let is_last = index == last_index;
            let index =
                u8::try_from(index).expect("we just checked that `iter.len() <= MAX_FRAGS`");
            let marker = if is_last {
                FragmentMarker::last(index)
            } else {
                FragmentMarker::non_last(index)
            }
            .expect("we just checked that `iter.len() <= MAX_FRAGS`");

            Fragment {
                header: FragmentHeader { msg_seq, marker },
                payload,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD_LEN: usize = 2;
    const MSG_SEQ: MessageSeq = MessageSeq::new(0);

    #[test]
    #[should_panic]
    fn panic_if_zero_payload_len() {
        let _ = FragmentSender::new(0);
    }

    #[test]
    fn msg_too_large() {
        let frag = FragmentSender::new(1);
        assert!(frag
            .fragment(MSG_SEQ, vec![1; MAX_FRAGS as usize].into())
            .is_ok());
        assert!(frag
            .fragment(MSG_SEQ, vec![1; MAX_FRAGS as usize + 1].into())
            .is_err())
    }

    #[test]
    fn msg_smaller_than_payload_len() {
        let frag = FragmentSender::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, vec![1].into()).unwrap();
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(0).unwrap(),
                },
                payload: Bytes::from_static(&[1]),
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }

    #[test]
    fn msg_equal_to_payload_len() {
        let frag = FragmentSender::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, vec![1, 2].into()).unwrap();
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(0).unwrap(),
                },
                payload: Bytes::from_static(&[1, 2]),
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }

    #[test]
    fn msg_larger_than_payload_len_1() {
        let frag = FragmentSender::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, vec![1, 2, 3].into()).unwrap();
        // remember, fragments are output in opposite index order
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(1).unwrap(),
                },
                payload: Bytes::from_static(&[3]),
            },
            frags.next().unwrap()
        );
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(0).unwrap(),
                },
                payload: Bytes::from_static(&[1, 2]),
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }

    #[test]
    fn msg_larger_than_payload_len_2() {
        let frag = FragmentSender::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, vec![1, 2, 3, 4, 5].into()).unwrap();
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::last(2).unwrap(),
                },
                payload: Bytes::from_static(&[5]),
            },
            frags.next().unwrap()
        );
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(1).unwrap(),
                },
                payload: Bytes::from_static(&[3, 4]),
            },
            frags.next().unwrap()
        );
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    msg_seq: MSG_SEQ,
                    marker: FragmentMarker::non_last(0).unwrap(),
                },
                payload: Bytes::from_static(&[1, 2]),
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }
}
