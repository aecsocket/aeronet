use std::{array, mem};

#[derive(Debug, Clone)]
pub struct SparseBuffer<T, const N: usize> {
    sparse: [usize; N],
    dense: [T; N],
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
    pub fn with_values(values: [T; N]) -> Self {
        assert!(N > 0);
        Self {
            sparse: [INVALID; N],
            dense: values,
        }
    }

    pub fn from_fn<F>(f: F) -> Self
    where
        F: FnMut(usize) -> T,
    {
        Self::with_values(array::from_fn(f))
    }

    pub fn from_clone(default: T) -> Self
    where
        T: Clone,
    {
        Self::from_fn(|_| default.clone())
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        let dense_index = index % N;
        if dense_index == INVALID {
            None
        } else {
            Some(&self.dense[dense_index])
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let dense_index = index % N;
        if dense_index == INVALID {
            None
        } else {
            Some(&mut self.dense[dense_index])
        }
    }

    pub fn insert(&mut self, index: usize, value: T) -> &mut T {
        let dense_index = index % N;
        self.dense[dense_index] = value;
        &mut self.dense[dense_index]
    }

    pub fn remove(&mut self, index: usize) {
        self.sparse[index] = INVALID;
    }

    pub fn replace(&mut self, index: usize, src: T) -> Option<T> {
        let dense_index = index % N;
        if dense_index == INVALID {
            None
        } else {
            Some(mem::replace(&mut self.dense[dense_index], src))
        }
    }

    pub fn take(&mut self, index: usize) -> Option<T>
    where
        T: Default,
    {
        self.replace(index, T::default())
    }
}
