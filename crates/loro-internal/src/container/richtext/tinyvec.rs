#[derive(Debug, Clone)]
pub(crate) struct TinyVec<T, const N: usize> {
    len: u8,
    data: [T; N],
}

impl<T: Default + Copy, const N: usize> TinyVec<T, N> {
    pub fn new() -> Self {
        if N > u8::MAX as usize {
            panic!("TinyVec size too large");
        }

        Self {
            len: 0,
            data: [Default::default(); N],
        }
    }

    pub fn push(&mut self, value: T) {
        if self.len == N as u8 {
            panic!("TinyVec is full");
        }

        self.data[self.len as usize] = value;
        self.len += 1;
    }

    pub fn get(&self, index: usize) -> &T {
        &self.data[index]
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            Some(self.data[self.len as usize].clone())
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter().take(self.len as usize)
    }

    pub fn can_merge(&self, other: &Self) -> bool {
        self.len + other.len <= self.data.len() as u8
    }

    pub fn merge(&mut self, other: &Self) {
        if !self.can_merge(other) {
            panic!("TinyVec cannot merge");
        }

        for value in other.iter() {
            self.push(*value);
        }
    }

    pub fn merge_left(&mut self, left: &Self) {
        if !self.can_merge(left) {
            panic!("TinyVec cannot merge");
        }

        for i in left.len()..N.min(self.len() + left.len()) {
            self.data[i] = self.data[i - left.len()];
        }

        self.data.copy_from_slice(&left.data[..left.len as usize]);
    }

    pub fn slice(&self, start: usize, end: usize) -> Self {
        let mut result = Self::new();

        for i in start..end {
            result.push(self.data[i]);
        }

        result
    }
}
