use core::iter::FusedIterator;

use bytes::Bytes;

/// Extension trait on types implementing [`Buf`] providing [`byte_chunks`].
///
/// [`Buf`]: bytes::Buf
/// [`byte_chunks`]: ByteChunksExt::byte_chunks
pub trait ByteChunksExt: Sized {
    /// Converts this into an iterator over non-overlapping chunks of the
    /// original bytes.
    ///
    /// # Examples
    ///
    /// With `len` being a multiple of `chunk_size`:
    ///
    /// ```
    /// # use bytes::Bytes;
    /// # use aeronet::octs::ByteChunksExt;
    /// let mut chunks = Bytes::from_static(&[1, 2, 3, 4]).byte_chunks(2);
    /// assert_eq!(&[1, 2], &*chunks.next().unwrap());
    /// assert_eq!(&[3, 4], &*chunks.next().unwrap());
    /// assert_eq!(None, chunks.next());
    /// ```
    ///
    /// With a remainder:
    ///
    /// ```
    /// # use bytes::Bytes;
    /// # use aeronet::octs::ByteChunksExt;
    /// let mut chunks = Bytes::from_static(&[1, 2, 3, 4, 5]).byte_chunks(2);
    /// assert_eq!(&[1, 2], &*chunks.next().unwrap());
    /// assert_eq!(&[3, 4], &*chunks.next().unwrap());
    /// assert_eq!(&[5], &*chunks.next().unwrap());
    /// assert_eq!(None, chunks.next());
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    fn byte_chunks(self, chunk_size: usize) -> ByteChunks<Self>;
}

impl<T> ByteChunksExt for T
where
    ByteChunks<T>: Iterator,
{
    fn byte_chunks(self, chunk_size: usize) -> ByteChunks<Self> {
        assert!(chunk_size > 0);
        ByteChunks {
            buf: self,
            chunk_size,
        }
    }
}

/// Iterator over [`Bytes`] of non-overlapping chunks, with each chunk being of
/// the same size.
///
/// The last item returned may not be of the same size as other items, as it may
/// return the remaining items.
///
/// Each [`Bytes`] returned is owned by the consumer, which is done by creating
/// a cheap clone of the underlying [`Bytes`], which just increases a reference
/// count and changes some indices.
///
/// Use [`byte_chunks`] to create.
///
/// See [`Chunks`].
///
/// [`byte_chunks`]: ByteChunksExt::byte_chunks
/// [`Chunks`]: core::slice::Chunks
#[derive(Debug)]
pub struct ByteChunks<T> {
    buf: T,
    chunk_size: usize,
}

impl<'a> Iterator for ByteChunks<&'a [u8]> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() {
            return None;
        }

        // copied from std::slice::Chunks
        let mid = self.buf.len().min(self.chunk_size);
        let (fst, snd) = self.buf.split_at(mid);
        self.buf = snd;
        Some(fst)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.buf.len() / self.chunk_size;
        let rem = self.buf.len() % self.chunk_size;
        let n = if rem > 0 { n + 1 } else { n };
        (n, Some(n))
    }

    #[inline]
    fn count(self) -> usize {
        self.len()
    }
}

impl ExactSizeIterator for ByteChunks<&[u8]> {}

impl FusedIterator for ByteChunks<&[u8]> {}

impl Iterator for ByteChunks<Bytes> {
    type Item = Bytes;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() {
            return None;
        }

        let mid = self.buf.len().min(self.chunk_size);
        let next = self.buf.split_to(mid);
        Some(next)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.buf.len() / self.chunk_size;
        let rem = self.buf.len() % self.chunk_size;
        let n = if rem > 0 { n + 1 } else { n };
        (n, Some(n))
    }

    #[inline]
    fn count(self) -> usize {
        self.len()
    }
}

impl ExactSizeIterator for ByteChunks<Bytes> {}

impl FusedIterator for ByteChunks<Bytes> {}
