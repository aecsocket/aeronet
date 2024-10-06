use {
    crate::ty::FragmentMarker,
    octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write},
    std::{convert::Infallible, fmt},
};

const LAST_MASK: u8 = 0b1000_0000;

/// Maximum index of any given fragment with a [`FragmentMarker`].
pub const MAX_FRAG_INDEX: u8 = u8::MAX & !LAST_MASK;

/// Maximum number of fragments that a message can be split into using
/// [`MessageSplitter`].
///
/// [`MessageSplitter`]: crate::msg::MessageSplitter
pub const MAX_FRAGS: usize = MAX_FRAG_INDEX as usize + 1;

impl FragmentMarker {
    /// Creates a new marker from a raw integer.
    #[inline]
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    /// Gets the raw integer from this fragment marker.
    ///
    /// To get the fragment index, use [`FragmentMarker::index`].
    #[inline]
    #[must_use]
    pub const fn into_raw(self) -> u8 {
        self.0
    }

    /// Creates a new marker from an index indicating that this **is not** the
    /// last fragment in the message.
    ///
    /// Returns [`None`] if the index is too large to be encoded properly.
    #[inline]
    #[must_use]
    pub const fn non_last(index: u8) -> Option<Self> {
        if index & LAST_MASK == 0 {
            Some(Self(index))
        } else {
            None
        }
    }

    /// Creates a new marker from an index indicating that this **is** the last
    /// fragment in the message.
    ///
    /// Returns [`None`] if the index is too large to be encoded properly.
    #[inline]
    #[must_use]
    pub const fn last(index: u8) -> Option<Self> {
        if index & LAST_MASK == 0 {
            Some(Self(index | LAST_MASK))
        } else {
            None
        }
    }

    /// Creates a new marker.
    ///
    /// If you know whether the marker is last or non-last at compile-time,
    /// prefer [`FragmentMarker::non_last`] or [`FragmentMarker::last`].
    #[inline]
    #[must_use]
    pub const fn new(index: u8, is_last: bool) -> Option<Self> {
        if is_last {
            Self::last(index)
        } else {
            Self::non_last(index)
        }
    }

    /// Gets the fragment index of this marker.
    #[inline]
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0 & !LAST_MASK
    }

    /// Gets if this fragment is the last one in the message.
    #[inline]
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.0 & LAST_MASK != 0
    }
}

impl fmt::Debug for FragmentMarker {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let index = self.index();
        if self.is_last() {
            write!(f, "FragmentMarker({index} last)")
        } else {
            write!(f, "FragmentMarker({index} non-last)")
        }
    }
}

impl FixedEncodeLen for FragmentMarker {
    const ENCODE_LEN: usize = u8::ENCODE_LEN;
}

impl Encode for FragmentMarker {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.0)
    }
}

impl Decode for FragmentMarker {
    type Error = Infallible;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self::from_raw(src.read()?))
    }
}

#[cfg(test)]
mod tests {
    use {super::*, octs::test::*};

    #[test]
    fn encode_decode_all_markers() {
        for raw in 0..u8::MAX {
            hint_round_trip(&FragmentMarker::from_raw(raw));
        }
    }

    #[test]
    fn non_last_index_in_range() {
        let marker = FragmentMarker::non_last(0).unwrap();
        assert!(!marker.is_last());
        assert_eq!(0, marker.index());

        let marker = FragmentMarker::non_last(MAX_FRAG_INDEX).unwrap();
        assert!(!marker.is_last());
        assert_eq!(MAX_FRAG_INDEX, marker.index());
    }

    #[test]
    fn last_index_in_range() {
        let marker = FragmentMarker::last(0).unwrap();
        assert!(marker.is_last());
        assert_eq!(0, marker.index());

        let marker = FragmentMarker::last(MAX_FRAG_INDEX).unwrap();
        assert!(marker.is_last());
        assert_eq!(MAX_FRAG_INDEX, marker.index());
    }

    #[test]
    fn out_of_range() {
        assert!(FragmentMarker::non_last(MAX_FRAG_INDEX + 1).is_none());
        assert!(FragmentMarker::last(MAX_FRAG_INDEX + 1).is_none());
    }
}
