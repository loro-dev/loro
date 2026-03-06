use std::{fmt::Debug, mem::MaybeUninit};

pub(crate) struct TinyVec<T, const N: usize> {
    len: u8,
    data: [MaybeUninit<T>; N],
}

impl<T: Debug, const N: usize> Debug for TinyVec<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.data[0..self.len as usize].iter())
            .finish()
    }
}

impl<T: Clone, const N: usize> Clone for TinyVec<T, N> {
    fn clone(&self) -> Self {
        let mut result = Self::new();
        // SAFETY: This is safe because we know that the data within range is initialized
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.data[..self.len()].as_ptr(),
                result.data.as_mut_ptr(),
                self.len as usize,
            );
        }

        result.len = self.len;
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
        if self.len + other.len > N as u8 {
            return false;
        }

        self.len + other.len <= self.data.len() as u8
    }

    pub fn merge(&mut self, other: &Self) {
        if !self.can_merge(other) {
            panic!("TinyVec cannot merge");
        }

        let start = self.len();
        // SAFETY: this is safe because we know that it's within the bounds
        unsafe {
            std::ptr::copy_nonoverlapping(
                other.data[..].as_ptr(),
                self.data[start..].as_mut_ptr(),
                other.len(),
            );
        }

        self.len += other.len;
    }

    pub fn merge_left(&mut self, left: &Self) {
        if !self.can_merge(left) {
            panic!("TinyVec cannot merge");
        }

        // SAFETY: this is safe because we know that it's within the bounds
        unsafe {
            std::ptr::copy(
                self.data[..].as_ptr(),
                self.data[left.len as usize..].as_mut_ptr(),
                self.len as usize,
            );
            std::ptr::copy_nonoverlapping(
                left.data[..].as_ptr(),
                self.data[..].as_mut_ptr(),
                left.len as usize,
            );
        }

        self.len += left.len;
    }

    pub fn slice(&self, start: usize, end: usize) -> Self {
        assert!(start <= end && end <= self.len as usize);
        let mut result = Self::new();
        if start == end {
            return result;
        }

        // SAFETY: This is safe because we know that the data within range is initialized
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.data[start..end].as_ptr(),
                result.data.as_mut_ptr(),
                end - start,
            );
        }

        result.len = (end - start) as u8;
        result
    }

    #[inline]
    pub fn split(&mut self, pos: usize) -> Self {
        let result = self.slice(pos, self.len());
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
}
