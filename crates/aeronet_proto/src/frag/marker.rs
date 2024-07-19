use std::convert::Infallible;

use octs::{BufTooShortOr, Decode, Encode, FixedEncodeLen, Read, Write};

use crate::ty::FragmentMarker;

const LAST_MASK: u8 = 0b1000_0000;

/// Maximum number of fragments that a message can be split into using
/// [`FragmentSender`].
///
/// See [`FragmentMarker`] for an explanation of how this value is determined.
pub const MAX_FRAGS: u8 = u8::MAX & !LAST_MASK;

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
    use octs::test::*;

    use super::*;

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

        let marker = FragmentMarker::non_last(MAX_FRAGS).unwrap();
        assert!(!marker.is_last());
        assert_eq!(MAX_FRAGS, marker.index());
    }

    #[test]
    fn last_index_in_range() {
        let marker = FragmentMarker::last(0).unwrap();
        assert!(marker.is_last());
        assert_eq!(0, marker.index());

        let marker = FragmentMarker::last(MAX_FRAGS).unwrap();
        assert!(marker.is_last());
        assert_eq!(MAX_FRAGS, marker.index());
    }

    #[test]
    fn out_of_range() {
        assert!(FragmentMarker::non_last(MAX_FRAGS + 1).is_none());
        assert!(FragmentMarker::last(MAX_FRAGS + 1).is_none());
    }
}
