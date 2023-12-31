//! Generic utilities.

use std::{array, mem};

/// Fixed-size sparse-set-like data structure for storage of a rolling buffer
/// of elements.
///
/// This buffer works by using `index % N` for querying the sparse buffer,
/// meaning that it is effectively circular.
#[derive(Debug, Clone)]
pub struct SparseBuffer<T, const N: usize> {
    sparse: Box<[usize; N]>,
    dense: Box<[T; N]>,
}

const INVALID: usize = usize::MAX;

impl<T, const N: usize> Default for SparseBuffer<T, N>
where
    T: Default,
{
    fn default() -> Self {
        Self::from_fn(|_| T::default())
    }
}

impl<T, const N: usize> SparseBuffer<T, N> {
    /// Creates a buffer, populating each underlying value using a function.
    ///
    /// The function takes in the current index and outputs a default value of
    /// `T`.
    ///
    /// # Panics
    ///
    /// Panics if `N` is 0 or less, or if `N` is [`u32::MAX`].
    #[must_use]
    pub fn from_fn<F>(f: F) -> Self
    where
        F: FnMut(usize) -> T,
    {
        assert!(N > 0);
        assert!(N < INVALID);
        Self {
            sparse: Box::new([INVALID; N]),
            dense: Box::new(array::from_fn(f)),
        }
    }

    /// Creates a buffer, populating each underlying value by cloning a default
    /// value.
    ///
    /// # Panics
    ///
    /// Panics if `N` is 0 or less, or if `N` is [`u32::MAX`].
    #[must_use]
    pub fn from_clone(default: T) -> Self
    where
        T: Clone,
    {
        Self::from_fn(|_| default.clone())
    }

    /// Gets a reference to a value in the buffer.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        let dense_index = index % N;
        if dense_index == INVALID {
            None
        } else {
            Some(&self.dense[dense_index])
        }
    }

    /// Gets a mutable reference to a value in the buffer.
    #[must_use]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let dense_index = index % N;
        if dense_index == INVALID {
            None
        } else {
            Some(&mut self.dense[dense_index])
        }
    }

    /// Inserts a value into this buffer.
    pub fn insert(&mut self, index: usize, value: T) -> &mut T {
        let dense_index = index % N;
        self.dense[dense_index] = value;
        &mut self.dense[dense_index]
    }

    /// Removes a value from this buffer.
    pub fn remove(&mut self, index: usize) {
        self.sparse[index] = INVALID;
    }

    /// Replaces a value in the buffer with another value, returning the
    /// previous value if there was one.
    pub fn replace(&mut self, index: usize, src: T) -> Option<T> {
        let dense_index = index % N;
        if dense_index == INVALID {
            None
        } else {
            Some(mem::replace(&mut self.dense[dense_index], src))
        }
    }

    /// Replaces a value in the buffer with the default value, returning the
    /// previous value if there was one.
    pub fn take(&mut self, index: usize) -> Option<T>
    where
        T: Default,
    {
        self.replace(index, T::default())
    }
}
