use {
    super::MAX_FRAGS,
    crate::ty::FragmentMarker,
    octs::{chunks::ByteChunksExt, Bytes},
    std::iter::FusedIterator,
};

/// Handles splitting large messages into multiple smaller fragments which can
/// be reassembled by a [`FragmentReceiver`].
///
/// [`FragmentReceiver`]: crate::msg::FragmentReceiver
#[derive(Debug, Clone, datasize::DataSize)]
pub struct MessageSplitter {
    max_payload_len: usize,
}

/// Attempted to split a message using [`MessageSplitter`] but the message was
/// too large to fit in [`MAX_FRAGS`] fragments.
#[derive(Debug, Clone, thiserror::Error)]
#[error("message too big - {len} / {max} bytes")]
pub struct MessageTooLarge {
    /// Length of the message in bytes.
    len: usize,
    /// Maximum length of the message in bytes.
    max: usize,
}

impl MessageSplitter {
    /// Creates a new [`MessageSplitter`].
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
        Self { max_payload_len }
    }

    /// Gets the maximum payload length.
    #[must_use]
    pub const fn max_payload_len(&self) -> usize {
        self.max_payload_len
    }

    /// Splits a message up into smaller fragments and creates per-fragment
    /// metadata for each ([`FragmentMarker`]), ready to be reassembled by a
    /// [`FragmentReceiver`].
    ///
    /// The message must be able to be split up into `MAX_FRAGS` or fewer
    /// fragments - that is, the message must not be larger than
    /// `MAX_FRAGS * max_payload_len`.
    ///
    /// Fragments are returned in the opposite order to the fragment index. If
    /// you pass a message which is split into fragments A, B, C, the iterator
    /// will return them in the order C, B, A.
    ///
    /// They should also be sent out along the transport in this reversed order.
    /// This is done to make reassembly more efficient, since when the receiver
    /// receives C (which is marked as the last fragment), it will immediately
    /// know how many fragments there are in total, and can allocate the right
    /// sized buffer to fit this message.
    ///
    /// Note that if this fragment C is lost, it will make reassembly slightly
    /// less efficient as the receiver will have to resize its buffer, but it
    /// will still behave correctly.
    ///
    /// # Errors
    ///
    /// Errors if the message is too large.
    ///
    /// [`FragmentReceiver`]: crate::msg::FragmentReceiver
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn split(
        &self,
        msg: impl Into<Bytes>,
    ) -> Result<
        impl ExactSizeIterator<Item = (FragmentMarker, Bytes)> + FusedIterator,
        MessageTooLarge,
    > {
        let msg = msg.into();
        let msg_len = msg.len();

        let iter = msg.byte_chunks(self.max_payload_len);
        let max_len = MAX_FRAGS * self.max_payload_len;
        if iter.len() > MAX_FRAGS {
            return Err(MessageTooLarge {
                len: msg_len,
                max: max_len,
            });
        }
        debug_assert!(msg_len <= max_len);

        let iter_len = iter.len();
        Ok(iter.enumerate().rev().map(move |(index, payload)| {
            // do this inside the iterator, since we now know
            // that we have at least at least 1 item in this iterator
            // and otherwise, `iter_len` would be 0, so `iter_len - 1`
            // would underflow
            let last_index = iter_len - 1;
            let is_last = index == last_index;
            let index = u8::try_from(index).expect(
                "`iter` has no more than `MAX_FRAGS` items, so `index` should be no more than \
                 `MAX_FRAG_INDEX`, so `index` should fit into a u8",
            );
            let marker = FragmentMarker::new(index, is_last)
                .expect("`index` should be no more than `MAX_FRAG_INDEX`");
            (marker, payload)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn non_last(index: u8) -> FragmentMarker {
        FragmentMarker::non_last(index).unwrap()
    }

    fn last(index: u8) -> FragmentMarker {
        FragmentMarker::last(index).unwrap()
    }

    #[test]
    #[should_panic]
    fn zero_payload_len() {
        let _ = MessageSplitter::new(0);
    }

    #[test]
    fn zero_msg_len() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[][..]).unwrap();
        assert!(fs.next().is_none());
    }

    #[test]
    fn half_frag() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[1][..]).unwrap();
        assert_eq!((last(0), vec![1].into()), fs.next().unwrap());
        assert!(fs.next().is_none());
    }

    #[test]
    fn one_frag() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[1, 2][..]).unwrap();
        assert_eq!((last(0), vec![1, 2].into()), fs.next().unwrap());
        assert!(fs.next().is_none());
    }

    #[test]
    fn one_and_half_frags() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[1, 2, 3][..]).unwrap();
        assert_eq!((last(1), vec![3].into()), fs.next().unwrap());
        assert_eq!((non_last(0), vec![1, 2].into()), fs.next().unwrap());
        assert!(fs.next().is_none());
    }

    #[test]
    fn two_frags() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[1, 2, 3, 4][..]).unwrap();
        assert_eq!((last(1), vec![3, 4].into()), fs.next().unwrap());
        assert_eq!((non_last(0), vec![1, 2].into()), fs.next().unwrap());
        assert!(fs.next().is_none());
    }

    #[test]
    fn two_half_frags() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[1, 2, 3, 4, 5][..]).unwrap();
        assert_eq!((last(2), vec![5].into()), fs.next().unwrap());
        assert_eq!((non_last(1), vec![3, 4].into()), fs.next().unwrap());
        assert_eq!((non_last(0), vec![1, 2].into()), fs.next().unwrap());
        assert!(fs.next().is_none());
    }

    #[test]
    fn three_frags() {
        let s = MessageSplitter::new(2);
        let mut fs = s.split(&[1, 2, 3, 4, 5, 6][..]).unwrap();
        assert_eq!((last(2), vec![5, 6].into()), fs.next().unwrap());
        assert_eq!((non_last(1), vec![3, 4].into()), fs.next().unwrap());
        assert_eq!((non_last(0), vec![1, 2].into()), fs.next().unwrap());
        assert!(fs.next().is_none());
    }

    #[test]
    fn too_large() {
        let s = MessageSplitter::new(1);
        let fs = s.split(&[1; MAX_FRAGS][..]).unwrap();
        assert_eq!(MAX_FRAGS, fs.len());

        assert!(s.split(&[1; MAX_FRAGS + 1][..]).is_err());
    }
}
