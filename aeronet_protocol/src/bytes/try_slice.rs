use std::ops::{Bound, RangeBounds};

use bytes::Bytes;

use super::ReadError;

/// Extension trait on [`Bytes`] providing [`TrySliceExt::try_slice`].
pub trait TrySliceExt {
    /// Returns a slice of self for the provided range.
    ///
    /// See [`Bytes::slice`].
    ///
    /// # Example
    ///
    /// ```
    /// # use bytes::Bytes;
    /// # use aeronet_protocol::bytes::TrySliceExt;
    /// let bytes = Bytes::from_static(&[1, 2, 3, 4, 5]);
    /// let slice1 = bytes.try_slice(1..3).unwrap();
    /// assert_eq!(&[2, 3], &*slice1);
    /// let slice2 = bytes.try_slice(..=3).unwrap();
    /// assert_eq!(&[1, 2, 3, 4], &*slice2);
    /// let slice3 = bytes.try_slice(..).unwrap();
    /// assert_eq!(bytes, slice3);
    ///
    /// bytes.try_slice(..10).unwrap_err();
    /// ```
    ///
    /// # Errors
    ///
    /// If the end of the range exceeds the length of this slice, this will error.
    ///
    /// # Panics
    ///
    /// Panics if the end bound of the range is an inclusive bound and its value
    /// is [`usize`], or if `begin > end`.
    fn try_slice(&self, range: impl RangeBounds<usize>) -> Result<Bytes, ReadError>;
}

impl TrySliceExt for Bytes {
    fn try_slice(&self, range: impl RangeBounds<usize>) -> Result<Bytes, ReadError> {
        let len = self.len();
        let end = match range.end_bound() {
            Bound::Included(&n) => n.checked_add(1).expect("out of range"),
            Bound::Excluded(&n) => n,
            Bound::Unbounded => len,
        };
        if end > len {
            return Err(ReadError::TooShort);
        }
        Ok(self.slice(range))
    }
}
