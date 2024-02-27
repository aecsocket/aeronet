use std::fmt::Debug;

#[derive(Clone, PartialEq, Eq)]
pub struct RollingBuf<T> {
    len: usize,
    items: Box<[Option<T>]>,
}

impl<T> RollingBuf<T> {
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0);
        Self {
            len: 0,
            items: (0..cap)
                .map(|_| None)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn capacity(&self) -> usize {
        self.items.len()
    }

    fn option(&self, index: usize) -> &Option<T> {
        let index = index % self.items.len();
        // SAFETY: the index is between [0, self.items.len())
        unsafe { self.items.get_unchecked(index) }
    }

    fn option_mut(&mut self, index: usize) -> &mut Option<T> {
        let index = index % self.items.len();
        // SAFETY: the index is between [0, self.items.len())
        unsafe { self.items.get_unchecked_mut(index) }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.option(index).as_ref()
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.option_mut(index).as_mut()
    }

    pub fn set(&mut self, index: usize, value: T) -> Option<T> {
        let index = index % self.items.len();
        let opt = self.option_mut(index);
        match opt.replace(value) {
            Some(old) => Some(old),
            None => {
                self.len += 1;
                None
            }
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        let index = index % self.items.len();
        self.option_mut(index).take()
    }

    pub fn entry(&mut self, index: usize) -> Entry<'_, T> {
        // SAFETY: the index is between [0, self.items.len())
        let option = unsafe { self.items.get_unchecked_mut(index) };
        Entry {
            len: &mut self.len,
            option,
        }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            iter: self.items.iter(),
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            iter: self.items.iter_mut(),
        }
    }

    pub fn clear(&mut self)
    where
        T: Clone,
    {
        self.items.fill(None)
    }

    pub fn retain(&mut self, mut f: impl FnMut(&T) -> bool) {
        self.retain_mut(|elem| f(elem))
    }

    pub fn retain_mut(&mut self, mut f: impl FnMut(&mut T) -> bool) {
        for item_opt in self.items.iter_mut() {
            let Some(item) = item_opt else { continue };
            if !f(item) {
                *item_opt = None;
            }
        }
    }
}

#[derive(Debug)]
pub struct Entry<'a, T> {
    len: &'a mut usize,
    option: &'a mut Option<T>,
}

impl<'a, T> Entry<'a, T> {
    pub fn or_insert(self, value: T) -> &'a mut T {
        self.or_insert_with(|| value)
    }

    pub fn or_insert_default(self) -> &'a mut T
    where
        T: Default,
    {
        self.or_insert_with(T::default)
    }

    pub fn or_insert_with(self, f: impl FnOnce() -> T) -> &'a mut T {
        self.option.get_or_insert_with(|| {
            *self.len += 1;
            f()
        })
    }
}

#[derive(Debug)]
pub struct Iter<'a, T> {
    iter: std::slice::Iter<'a, Option<T>>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(Option::as_ref).flatten()
    }
}

#[derive(Debug)]
pub struct IterMut<'a, T> {
    iter: std::slice::IterMut<'a, Option<T>>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(Option::as_mut).flatten()
    }
}

impl<T> IntoIterator for RollingBuf<T> {
    type Item = T;

    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            iter: self.items.into_vec().into_iter(),
        }
    }
}

#[derive(Debug)]
pub struct IntoIter<T> {
    iter: std::vec::IntoIter<Option<T>>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().flatten()
    }
}

impl<T: Debug> Debug for RollingBuf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::RollingBuf;

    #[test]
    fn new() {
        RollingBuf::<i32>::new(256);
    }
}
