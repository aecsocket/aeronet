use std::iter::FusedIterator;

use bytes::Bytes;

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
/// See [`std::slice::Chunks`].
///
/// [`byte_chunks`]: ByteChunksExt::byte_chunks
pub struct ByteChunks {
    v: Bytes,
    chunk_size: usize,
}

impl Iterator for ByteChunks {
    type Item = Bytes;

    fn next(&mut self) -> Option<Self::Item> {
        if self.v.is_empty() {
            None
        } else {
            let mid = self.v.len().min(self.chunk_size);
            let next = self.v.split_to(mid);
            Some(next)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.v.is_empty() {
            (0, Some(0))
        } else {
            let n = self.v.len() / self.chunk_size;
            let rem = self.v.len() % self.chunk_size;
            let n = if rem > 0 { n + 1 } else { n };
            (n, Some(n))
        }
    }

    fn count(self) -> usize {
        self.len()
    }
}

impl ExactSizeIterator for ByteChunks {}

impl FusedIterator for ByteChunks {}

/// Extension trait on [`Bytes`] to allow using [`ByteChunks`].
pub trait ByteChunksExt {
    /// Converts this into an iterator over non-overlapping chunks of the
    /// original bytes.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    fn byte_chunks(self, chunk_size: usize) -> ByteChunks;
}

impl ByteChunksExt for Bytes {
    fn byte_chunks(self, chunk_size: usize) -> ByteChunks {
        assert!(chunk_size > 0);
        ByteChunks {
            v: self,
            chunk_size,
        }
    }
}
