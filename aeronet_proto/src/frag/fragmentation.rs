use std::iter::FusedIterator;

use aeronet::octs;

use crate::seq::Seq;

use super::{Fragment, FragmentHeader};

/// Handles splitting a single large message into multiple smaller fragments
/// which can be reassembled by [`Reassembly`].
///
/// [`Reassembly`]: crate::frag::Reassembly
#[derive(Debug, Clone)]
pub struct Fragmentation {
    payload_len: usize,
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

impl Fragmentation {
    /// Creates a new [`Fragmentation`].
    ///
    /// * `payload_len` defines the maximum length, in bytes, that the payload
    ///   of a single fragmented packet can be. This must be greater than 0.
    ///
    /// # Panics
    ///
    /// Panics if `payload_len` is 0.
    pub fn new(payload_len: usize) -> Self {
        assert!(payload_len > 0);
        Self { payload_len }
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
    pub fn fragment<B>(&self, msg_seq: Seq, msg: B) -> Result<Fragments<B>, FragmentError>
    where
        B: bytes::Buf + octs::ByteChunksExt,
        octs::ByteChunks<B>: ExactSizeIterator,
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
}

/// Iterator over fragments created by [`FragmentSender::fragment`].
#[derive(Debug)]
pub struct Fragments<B> {
    msg_seq: Seq,
    num_frags: u8,
    iter: std::iter::Enumerate<octs::ByteChunks<B>>,
}

impl<B> Fragments<B> {
    /// Gets the number of fragments that this iterator produces in total.
    pub fn num_frags(&self) -> u8 {
        self.num_frags
    }
}

impl<T, U> Iterator for Fragments<T>
where
    octs::ByteChunks<T>: Iterator<Item = U>,
{
    type Item = Fragment<U>;

    fn next(&mut self) -> Option<Self::Item> {
        let (frag_id, payload) = self.iter.next()?;
        let frag_id =
            u8::try_from(frag_id).expect("`num_frags` is a u8, so `frag_id` should be convertible");
        let header = FragmentHeader {
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

impl<T, U> ExactSizeIterator for Fragments<T> where octs::ByteChunks<T>: ExactSizeIterator<Item = U> {}

impl<T, U> FusedIterator for Fragments<T> where octs::ByteChunks<T>: FusedIterator<Item = U> {}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD_LEN: usize = 2;
    const MSG_SEQ: Seq = Seq(0);

    #[test]
    #[should_panic]
    fn panic_if_zero_payload_len() {
        Fragmentation::new(0);
    }

    #[test]
    fn msg_smaller_than_payload_len() {
        let frag = Fragmentation::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, &[1][..]).unwrap();
        let header = FragmentHeader {
            msg_seq: MSG_SEQ,
            num_frags: 1,
            frag_id: 0,
        };
        assert_eq!(
            Fragment {
                header,
                payload: &[1][..],
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }

    #[test]
    fn msg_equal_to_payload_len() {
        let frag = Fragmentation::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, &[1, 2][..]).unwrap();
        let header = FragmentHeader {
            msg_seq: MSG_SEQ,
            num_frags: 1,
            frag_id: 0,
        };
        assert_eq!(
            Fragment {
                header,
                payload: &[1, 2][..],
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }

    #[test]
    fn msg_larger_than_payload_len_1() {
        let frag = Fragmentation::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, &[1, 2, 3][..]).unwrap();
        let header = FragmentHeader {
            msg_seq: MSG_SEQ,
            num_frags: 2,
            frag_id: 0,
        };
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    frag_id: 0,
                    ..header
                },
                payload: &[1, 2][..],
            },
            frags.next().unwrap()
        );
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    frag_id: 1,
                    ..header
                },
                payload: &[3][..],
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }

    #[test]
    fn msg_larger_than_payload_len_2() {
        let frag = Fragmentation::new(PAYLOAD_LEN);
        let mut frags = frag.fragment(MSG_SEQ, &[1, 2, 3, 4, 5][..]).unwrap();
        let header = FragmentHeader {
            msg_seq: MSG_SEQ,
            num_frags: 3,
            frag_id: 0,
        };
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    frag_id: 0,
                    ..header
                },
                payload: &[1, 2][..],
            },
            frags.next().unwrap()
        );
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    frag_id: 1,
                    ..header
                },
                payload: &[3, 4][..],
            },
            frags.next().unwrap()
        );
        assert_eq!(
            Fragment {
                header: FragmentHeader {
                    frag_id: 2,
                    ..header
                },
                payload: &[5][..],
            },
            frags.next().unwrap()
        );
        assert!(frags.next().is_none());
    }
}
