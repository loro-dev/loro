use std::{fmt::Debug, mem::MaybeUninit};

pub(crate) struct TinyVec<T, const N: usize> {
    len: u8,
    data: [MaybeUninit<T>; N],
}

impl<T: Debug, const N: usize> Debug for TinyVec<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Clone, const N: usize> Clone for TinyVec<T, N> {
    fn clone(&self) -> Self {
        let mut result = Self::new();
        for item in self.iter() {
            result
                .push(item.clone())
                .ok()
                .expect("TinyVec clone should fit in the same capacity");
        }
        result
    }
}

impl<T, const N: usize> std::ops::Index<usize> for TinyVec<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        assert!(index < self.len as usize);
        // SAFETY: This is safe because we know that the data is initialized
        unsafe { self.data[index].assume_init_ref() }
    }
}

impl<T, const N: usize> TinyVec<T, N> {
    #[inline]
    pub fn new() -> Self {
        if N > u8::MAX as usize {
            panic!("TinyVec size too large");
        }

        Self {
            len: 0,
            // SAFETY: This initialization is copied from std
            // SAFETY: An uninitialized `[MaybeUninit<_>; LEN]` is valid.
            data: unsafe { MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init() },
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.len == N as u8 {
            return Err(value);
        }

        self.data[self.len as usize] = MaybeUninit::new(value);
        self.len += 1;
        Ok(())
    }

    #[inline(always)]
    pub fn get(&self, index: usize) -> &T {
        &self[index]
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            // SAFETY: This is safe because we know that the last element is initialized
            Some(unsafe { self.data[self.len as usize].assume_init_read() })
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let mut index = 0;
        std::iter::from_fn(move || {
            if index < self.len as usize {
                let result = &self[index];
                index += 1;
                Some(result)
            } else {
                None
            }
        })
    }

    pub fn can_merge(&self, other: &Self) -> bool {
        if self.len() + other.len() > N {
            return false;
        }

        self.len() + other.len() <= self.data.len()
    }

    pub fn merge(&mut self, other: &Self)
    where
        T: Clone,
    {
        if !self.can_merge(other) {
            panic!("TinyVec cannot merge");
        }

        for item in other.iter() {
            self.push(item.clone())
                .ok()
                .expect("TinyVec merge should fit after can_merge");
        }
    }

    pub fn merge_left(&mut self, left: &Self)
    where
        T: Clone,
    {
        if !self.can_merge(left) {
            panic!("TinyVec cannot merge");
        }

        let mut result = Self::new();
        for item in left.iter().chain(self.iter()) {
            result
                .push(item.clone())
                .ok()
                .expect("TinyVec merge_left should fit after can_merge");
        }
        *self = result;
    }

    pub fn slice(&self, start: usize, end: usize) -> Self
    where
        T: Clone,
    {
        assert!(start <= end && end <= self.len as usize);
        let mut result = Self::new();
        for item in self.iter().skip(start).take(end - start) {
            result
                .push(item.clone())
                .ok()
                .expect("TinyVec slice should fit in the same capacity");
        }
        result
    }

    #[inline]
    pub fn split(&mut self, pos: usize) -> Self {
        assert!(pos <= self.len());
        let old_len = self.len();
        let mut result = Self::new();
        for i in pos..old_len {
            // SAFETY: indexes below old_len are initialized. The value is moved
            // into result and self.len is truncated below before self is dropped.
            let value = unsafe { self.data[i].assume_init_read() };
            result
                .push(value)
                .ok()
                .expect("TinyVec split should fit in the same capacity");
        }
        self.len = pos as u8;
        result
    }
}

impl<T: Clone, const N: usize> TinyVec<T, N> {
    pub fn to_vec(&self) -> Vec<T> {
        let mut result = Vec::with_capacity(self.len as usize);
        for i in 0..self.len as usize {
            result.push(self[i].clone());
        }
        result
    }
}

impl<T: PartialEq, const N: usize> PartialEq for TinyVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }

        for i in 0..self.len as usize {
            if self[i] != other[i] {
                return false;
            }
        }

        true
    }
}

impl<T: Eq, const N: usize> Eq for TinyVec<T, N> {}

impl<T, const N: usize> Drop for TinyVec<T, N> {
    fn drop(&mut self) {
        for i in 0..self.len as usize {
            // SAFETY: This is safe because we know that the element is initialized
            unsafe { std::ptr::drop_in_place(self.data[i].as_mut_ptr()) };
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::TinyVec;

    #[test]
    fn pop_should_drop() {
        let mut arr: TinyVec<_, 2> = TinyVec::new();
        let elem = Arc::new(10);
        assert_eq!(Arc::strong_count(&elem), 1);
        arr.push(elem.clone()).unwrap();
        assert_eq!(Arc::strong_count(&elem), 2);
        arr.pop();
        assert_eq!(Arc::strong_count(&elem), 1);
    }

    #[test]
    fn dropping_should_drop_all_elem() {
        let mut arr: TinyVec<_, 2> = TinyVec::new();
        let elem = Arc::new(10);
        assert_eq!(Arc::strong_count(&elem), 1);
        arr.push(elem.clone()).unwrap();
        assert_eq!(Arc::strong_count(&elem), 2);
        drop(arr);
        assert_eq!(Arc::strong_count(&elem), 1);
    }

    #[test]
    fn split() {
        let mut arr: TinyVec<u8, 8> = TinyVec::new();
        arr.push(1).unwrap();
        arr.push(2).unwrap();
        arr.push(3).unwrap();
        arr.push(4).unwrap();
        let new = arr.split(2);
        assert_eq!(arr.to_vec(), vec![1, 2]);
        assert_eq!(new.to_vec(), vec![3, 4]);
    }

    #[test]
    fn slice() {
        let mut arr: TinyVec<u8, 8> = TinyVec::new();
        arr.push(1).unwrap();
        arr.push(2).unwrap();
        arr.push(3).unwrap();
        arr.push(4).unwrap();
        arr.push(5).unwrap();
        let arr = arr;
        let new = arr.slice(2, 4);
        assert_eq!(new.to_vec(), vec![3, 4]);
    }

    #[test]
    fn clone() {
        let mut arr: TinyVec<u8, 8> = TinyVec::new();
        arr.push(1).unwrap();
        arr.push(2).unwrap();
        arr.push(3).unwrap();
        arr.push(4).unwrap();
        arr.push(5).unwrap();
        let arr = arr;
        let new = arr.clone();
        assert_eq!(arr, new);
        assert_eq!(new.to_vec(), [1, 2, 3, 4, 5]);
    }

    #[cfg(miri)]
    #[test]
    fn clone_of_owned_values_does_not_duplicate_ownership_with_raw_copy() {
        let mut arr: TinyVec<Box<i32>, 2> = TinyVec::new();
        arr.push(Box::new(1)).unwrap();

        let cloned = arr.clone();
        drop(arr);
        drop(cloned);
    }
}
