//! See [`SeqBuf`].

use {
    alloc::boxed::Box,
    core::{array, mem},
    typesize::TypeSize,
};

/// Rolling sequence buffer data structure.
///
/// This provides constant-time insertion, query, access, and removal of items
/// which have a monotonically increasing integer sequence number with a value
/// up to [`u16::MAX`]. This is achieved by using two arrays:
/// - `indices`, an array of sequence numbers
/// - `data`, an array of the `T`s holding the actual data
///
/// When accessing into this buffer using the key `k`, an index `i` is computed
/// using `k % N`. We store `k` at `indices[i]`, and store the actual `T` at
/// `data[i]`. This means that **multiple keys will map to the same index**, so
/// inserting a value and attempting to access it later may result in reading a
/// different value than the one inserted.
/// To get around this, when accessing a value we check that `indices[i] == k`,
/// indicating that a new value has not been inserted into this index in the
/// meantime, and only then provide access to `data[i]`.
///
/// To avoid `unsafe` usage, all elements of `data` must be populated with valid
/// values. You will need a way to construct a valid (if meaningless) `T` when
/// creating the buffer or removing elements. If `T: Default`, functions are
/// provided to use the default value in these cases (such as [`SeqBuf::new`]).
///
/// This implementation is based on the article in [*Gaffer On Games*].
///
/// [*Gaffer On Games*]: https://gafferongames.com/post/reliable_ordered_messages#sequence-buffers
// TODO:
// The solution to this problem is to walk between the previous highest insert sequence and the new
// insert sequence (if it is more recent) and clear those entries in the sequence buffer to
// 0xFFFFFFFF. Now in the common case, insert is very close to constant time, but worst
// case is linear where n is the number of sequence entries between the previous highest insert
// sequence and the current insert sequence.
#[derive(Debug)]
pub struct SeqBuf<T, const N: usize> {
    indices: Box<[u16; N]>,
    data: Box<[T; N]>,
    len: usize,
}

const EMPTY: u16 = u16::MAX;

impl<T: Default, const N: usize> Default for SeqBuf<T, N> {
    fn default() -> Self {
        Self::new_from_fn(|_| T::default())
    }
}

impl<T, const N: usize> SeqBuf<T, N> {
    /// Creates a new sequence buffer, populating the data array with items
    /// given by the callback.
    ///
    /// If `T: Default`, consider using [`SeqBuf::new`].
    ///
    /// # Panics
    ///
    /// Panics if `N == 0` or `N >= u16::MAX`.
    #[must_use]
    pub fn new_from_fn(cb: impl FnMut(usize) -> T) -> Self {
        assert!(N > 0);
        assert!(N < u16::MAX as usize);
        Self {
            indices: Box::new([EMPTY; N]),
            data: Box::new(array::from_fn(cb)),
            len: 0,
        }
    }

    /// Gets the number of elements in this sequence buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::seq_buf::SeqBuf;
    ///
    /// let mut buf = SeqBuf::<String, 16>::new();
    /// assert_eq!(0, buf.len());
    ///
    /// buf.insert(3, "hi #1".into());
    /// assert_eq!(1, buf.len());
    ///
    /// buf.insert(5, "bye".into());
    /// assert_eq!(2, buf.len());
    ///
    /// buf.insert(3, "hi #2".into());
    /// assert_eq!(2, buf.len());
    ///
    /// buf.remove(3);
    /// assert_eq!(1, buf.len());
    ///
    /// buf.remove(5);
    /// assert_eq!(0, buf.len());
    /// ```
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if there are no elements in this sequence buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::seq_buf::SeqBuf;
    ///
    /// let mut buf = SeqBuf::<String, 16>::new();
    /// assert!(buf.is_empty());
    ///
    /// buf.insert(0, "hi".into());
    /// assert!(!buf.is_empty());
    ///
    /// buf.remove(0);
    /// assert!(buf.is_empty());
    /// ```
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn index(key: u16) -> u16 {
        #[expect(clippy::cast_possible_truncation, reason = "N < u16::MAX")]
        let index = key % N as u16;
        debug_assert!(index != EMPTY);
        index
    }

    /// Gets a reference to the item at the given key.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::seq_buf::SeqBuf;
    ///
    /// let mut buf = SeqBuf::<String, 16>::new();
    /// assert!(buf.get(7).is_none());
    ///
    /// buf.insert(7, "hello world".into());
    /// assert_eq!("hello world", buf.get(7).unwrap());
    ///
    /// buf.remove(7);
    /// assert!(buf.get(7).is_none());
    /// ```
    #[must_use]
    #[inline]
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn get(&self, key: u16) -> Option<&T> {
        let index = Self::index(key);
        let index_u = usize::from(index);
        let real_index = *self.indices.get(index_u).expect("key % N should be < N");
        if key == real_index {
            Some(self.data.get(index_u).expect(
                "`index_u` is valid into `indices`, and `indices` is of the same length as \
                 `data`, so it should be a valid index into `data`",
            ))
        } else {
            None
        }
    }

    /// Gets a mutable reference to the item at the given key.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::seq_buf::SeqBuf;
    ///
    /// let mut buf = SeqBuf::<String, 16>::new();
    /// buf.insert(7, "hello world".into());
    ///
    /// *buf.get_mut(7).unwrap() = "goodbye world".into();
    /// assert_eq!("goodbye world", buf.get(7).unwrap());
    /// ```
    #[must_use]
    #[inline]
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn get_mut(&mut self, key: u16) -> Option<&mut T> {
        let index = Self::index(key);
        let index_u = usize::from(index);
        let real_index = *self.indices.get(index_u).expect("key % N should be < N");
        if key == real_index {
            Some(self.data.get_mut(index_u).expect(
                "`index_u` is valid into `indices`, and `indices` is of the same length as \
                 `data`, so it should be a valid index into `data`",
            ))
        } else {
            None
        }
    }

    /// Inserts a value into this buffer at the given key, overwriting any value
    /// previously stored at that key.
    ///
    /// If this key is greater than `N`, this may overwrite a value previously
    /// stored at a different key. More specifically, this will overwrite the
    /// value stored at the index `key % N`. For example, if `N = 16`, then all
    /// of the following keys will write into the same index, and overwrite the
    /// same value:
    /// - 1
    /// - 17 (1 + 16)
    /// - 33 (1 + 16 + 16)
    /// - 49 (1 + 16 + 16 + 16)
    ///
    /// Returns a reference to the newly inserted value.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::seq_buf::SeqBuf;
    ///
    /// let mut buf = SeqBuf::<String, 16>::new();
    /// let inserted = buf.insert(4, "hello world".into());
    /// assert_eq!("hello world", inserted);
    /// assert_eq!(1, buf.len());
    ///
    /// let inserted = buf.insert(4, "hello".into());
    /// assert_eq!("hello", inserted);
    /// assert_eq!(1, buf.len());
    ///
    /// let inserted = buf.insert(4 + 16, "world".into());
    /// assert_eq!("world", inserted);
    /// assert_eq!(1, buf.len());
    /// ```
    #[inline]
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn insert(&mut self, key: u16, value: T) -> &mut T {
        let index = Self::index(key);
        let index_u = usize::from(index);
        let index_slot = self
            .indices
            .get_mut(index_u)
            .expect("key % N should be < N");
        if *index_slot == EMPTY {
            self.len = self
                .len
                .checked_add(1)
                .expect("`len` should never go above `usize::MAX`");
        }
        *index_slot = key;

        let data_slot = self.data.get_mut(index_u).expect(
            "`index_u` is valid into `indices`, and `indices` is of the same length as `data`, so \
             it should be a valid index into `data`",
        );
        *data_slot = value;
        data_slot
    }

    /// Removes a value from this buffer at the given key, replacing it with a
    /// default (meaningless) value.
    ///
    /// If `T: Default`, consider using [`SeqBuf::remove`].
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::seq_buf::SeqBuf;
    ///
    /// let mut buf = SeqBuf::<String, 16>::new();
    /// buf.insert(4, "hello world".into());
    /// assert_eq!(1, buf.len());
    ///
    /// let removed = buf.remove_with(4, String::new()).unwrap();
    /// assert_eq!("hello world", removed);
    /// assert_eq!(0, buf.len());
    /// ```
    #[inline]
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn remove_with(&mut self, key: u16, default: T) -> Option<T> {
        let index = Self::index(key);
        let index_u = usize::from(index);
        let index_slot = self
            .indices
            .get_mut(index_u)
            .expect("key % N should be < N");

        if *index_slot != EMPTY && key == *index_slot {
            *index_slot = EMPTY;
            let data_slot = self.data.get_mut(index_u).expect(
                "`index_u` is valid into `indices`, and `indices` is of the same length as \
                 `data`, so it should be a valid index into `data`",
            );
            self.len = self
                .len
                .checked_sub(1)
                .expect("`len` should never drop below 0");
            Some(mem::replace(data_slot, default))
        } else {
            None
        }
    }
}

impl<T: TypeSize, const N: usize> TypeSize for SeqBuf<T, N> {
    fn extra_size(&self) -> usize {
        self.indices.extra_size() + self.data.extra_size()
    }
}

impl<T: Default, const N: usize> SeqBuf<T, N> {
    /// Creates a new sequence buffer, populating the data array with default
    /// values of `T`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes a value from this buffer at the given key.
    ///
    /// See [`SeqBuf::remove_with`].
    #[inline]
    pub fn remove(&mut self, key: u16) -> Option<T> {
        self.remove_with(key, T::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic = "assertion failed: N > 0"]
    fn zero_cap() {
        let _ = SeqBuf::<(), 0>::new();
    }

    #[test]
    #[should_panic = "assertion failed: N < u16::MAX as usize"]
    fn over_max_cap() {
        let _ = SeqBuf::<(), { u16::MAX as usize }>::new();
    }

    #[test]
    fn max_cap() {
        let _ = SeqBuf::<(), { u16::MAX as usize - 1 }>::new();
    }

    #[test]
    fn single() {
        let mut b = SeqBuf::<u32, 16>::new();
        assert!(b.get(0).is_none());

        b.insert(0, 1234);
        assert_eq!(1234, *b.get(0).unwrap());
        assert_eq!(1234, *b.get_mut(0).unwrap());

        assert_eq!(1234, b.remove(0).unwrap());
        assert!(b.get(0).is_none());
        assert!(b.get_mut(0).is_none());
        assert!(b.remove(0).is_none());
    }

    #[test]
    fn keys_lower_than_cap() {
        let mut b = SeqBuf::<u32, 16>::new();

        b.insert(0, 12);
        b.insert(1, 34);
        b.insert(5, 56);
        b.insert(10, 78);

        assert_eq!(12, *b.get(0).unwrap());
        assert_eq!(34, *b.get(1).unwrap());
        assert_eq!(56, *b.get(5).unwrap());
        assert_eq!(78, *b.get(10).unwrap());

        assert_eq!(12, b.remove(0).unwrap());
        assert_eq!(34, b.remove(1).unwrap());
        assert_eq!(56, b.remove(5).unwrap());
        assert_eq!(78, b.remove(10).unwrap());
    }

    #[test]
    fn keys_higher_than_cap() {
        let mut b = SeqBuf::<u32, 16>::new();

        b.insert(10, 12);
        b.insert(15, 34);
        b.insert(16, 56);
        b.insert(17, 78);

        assert_eq!(12, *b.get(10).unwrap());
        assert_eq!(34, *b.get(15).unwrap());
        assert_eq!(56, *b.get(16).unwrap());
        assert_eq!(78, *b.get(17).unwrap());

        assert_eq!(12, b.remove(10).unwrap());
        assert_eq!(34, b.remove(15).unwrap());
        assert_eq!(56, b.remove(16).unwrap());
        assert_eq!(78, b.remove(17).unwrap());
    }

    #[test]
    fn overwrite() {
        let mut b = SeqBuf::<u32, 16>::new();

        b.insert(0, 111);
        b.insert(16, 222);

        // we lose `111` since we overwrite that slot with `222`
        assert!(b.get(0).is_none());
        assert_eq!(222, *b.get(16).unwrap());
    }

    #[test]
    fn u16_max_key() {
        let mut b = SeqBuf::<u32, 16>::new();

        assert!(b.remove(u16::MAX).is_none());
        assert!(b.is_empty());

        b.insert(u16::MAX, 1);
        assert_eq!(1, *b.get(u16::MAX).unwrap());
        assert_eq!(1, b.len());
    }
}
