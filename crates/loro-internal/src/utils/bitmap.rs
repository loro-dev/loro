#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitMap {
    vec: Vec<u8>,
    len: usize,
}

impl BitMap {
    pub fn new() -> Self {
        Self {
            vec: Vec::new(),
            len: 0,
        }
    }

    pub fn from_vec(vec: Vec<u8>, len: usize) -> Self {
        Self { vec, len }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.vec
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, v: bool) {
        if self.len % 8 == 0 {
            self.vec.push(0);
        }
        if v {
            self.vec[self.len / 8] |= 1 << (self.len % 8);
        }
        self.len += 1;
    }

    pub fn get(&self, index: usize) -> bool {
        if index >= self.len {
            panic!("index out of range");
        }
        self.vec[index / 8] & (1 << (index % 8)) != 0
    }
}
