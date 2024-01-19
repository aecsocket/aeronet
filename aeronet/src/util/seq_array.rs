use std::{mem::MaybeUninit, ops};

pub struct SeqArray<T, const N: usize> {
    valid: Box<[bool; N]>,
    items: Box<[MaybeUninit<T>; N]>,
    len: usize,
}

impl<T, const N: usize> Default for SeqArray<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> SeqArray<T, N> {
    pub fn new() -> Self {
        assert!(N > 0);
        Self {
            valid: Box::new([false; N]),
            items: Box::new(std::array::from_fn(|_| MaybeUninit::uninit())),
            len: 0,
        }
    }

    pub fn contains(&self, index: usize) -> bool {
        let index = index % N;
        // SAFETY: `index` is between 0 and N
        unsafe { *self.valid.get_unchecked(index) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn insert(&mut self, index: usize, value: T) -> Option<T> {
        let index = index % N;
        if self.contains(index) {
            // SAFETY: `index` is between 0 and N
            let item_ref = unsafe { self.items.get_unchecked_mut(index) };
            // SAFETY: This index already `contains` a value, so it was already
            // written to previously, and this value will be initialized
            let item = unsafe { item_ref.assume_init_read() };
            item_ref.write(value);
            Some(item)
        } else {
            // SAFETY: `index` is between 0 and N
            unsafe {
                *self.valid.get_unchecked_mut(index) = true;
                self.items.get_unchecked_mut(index).write(value);
            }
            self.len += 1;
            None
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        let index = index % N;
        if self.contains(index) {
            // SAFETY: `index` is between 0 and N
            unsafe {
                *self.valid.get_unchecked_mut(index) = false;
            }
            // SAFETY: This index already `contains` a value, so it was already
            // written to previously, and this value will be initialized
            let item = unsafe { self.items.get_unchecked_mut(index).assume_init_read() };
            self.len -= 1;
            Some(item)
        } else {
            None
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        let index = index % N;
        if self.contains(index) {
            // SAFETY: This index already `contains` a value, so it was already
            // written to previously, and this value will be initialized
            let item = unsafe { self.items.get_unchecked(index).assume_init_ref() };
            Some(item)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let index = index % N;
        if self.contains(index) {
            // SAFETY: This index already `contains` a value, so it was already
            // written to previously, and this value will be initialized
            let item = unsafe { self.items.get_unchecked_mut(index).assume_init_mut() };
            Some(item)
        } else {
            None
        }
    }
}

impl<T, const N: usize> ops::Index<usize> for SeqArray<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap()
    }
}

impl<T, const N: usize> ops::IndexMut<usize> for SeqArray<T, N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let mut arr = SeqArray::<char, 4>::new();
        assert_eq!(0, arr.len());
        assert!(arr.is_empty());

        assert_eq!(None, arr.get(0));
        assert_eq!(None, arr.get_mut(0));
        assert_eq!(None, arr.remove(0));
    }

    #[test]
    fn one() {
        let mut arr = SeqArray::<char, 4>::new();
        assert_eq!(None, arr.insert(0, 'a'));
        assert_eq!(1, arr.len());

        assert_eq!(Some(&'a'), arr.get(0));
        assert_eq!(Some(&mut 'a'), arr.get_mut(0));
        assert_eq!(Some('a'), arr.remove(0));

        assert_eq!(None, arr.get(0));
        assert_eq!(None, arr.get_mut(0));
        assert_eq!(None, arr.remove(0));
    }

    #[test]
    fn replace() {
        let mut arr = SeqArray::<char, 4>::new();
        assert_eq!(None, arr.insert(0, 'a'));
        assert_eq!(Some('a'), arr.insert(0, 'b'));
        assert_eq!(Some('b'), arr.insert(0, 'c'));
        assert_eq!(1, arr.len());
    }

    #[test]
    fn two() {
        let mut arr = SeqArray::<char, 4>::new();
        assert_eq!(None, arr.insert(0, 'a'));
        assert_eq!(None, arr.insert(1, 'b'));
        assert_eq!(2, arr.len());

        assert_eq!(Some(&'a'), arr.get(0));
        assert_eq!(Some(&mut 'a'), arr.get_mut(0));

        assert_eq!(Some(&'b'), arr.get(1));
        assert_eq!(Some(&mut 'b'), arr.get_mut(1));
    }

    #[test]
    fn overflow() {
        let mut arr = SeqArray::<char, 4>::new();
        assert_eq!(None, arr.insert(0, 'a'));
        assert_eq!(Some('a'), arr.insert(4, 'b'));
    }

    #[test]
    fn zst() {
        let mut arr = SeqArray::<(), 4>::new();
        assert_eq!(None, arr.insert(0, ()));
        assert_eq!(Some(()), arr.remove(0));
    }
}
