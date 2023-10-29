#[derive(Clone, PartialEq, Eq)]
pub struct BitMap {
    vec: Vec<u8>,
    len: usize,
}

impl std::fmt::Debug for BitMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ans = String::new();
        for v in self.iter() {
            if v {
                ans.push('1');
            } else {
                ans.push('0');
            }
        }

        f.debug_struct("BitMap")
            .field("len", &self.len)
            .field("vec", &ans)
            .finish()
    }
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

    #[allow(unused)]
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, v: bool) {
        while self.len / 8 >= self.vec.len() {
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

        (self.vec[index / 8] & (1 << (index % 8))) != 0
    }

    pub fn iter(&self) -> impl Iterator<Item = bool> + '_ {
        self.vec
            .iter()
            .flat_map(|&v| (0..8).map(move |i| (v & (1 << i)) != 0))
            .take(self.len)
    }
}

#[cfg(test)]
mod test {
    use super::BitMap;

    #[test]
    fn basic() {
        let mut map = BitMap::new();
        map.push(true);
        map.push(false);
        map.push(true);
        map.push(true);
        assert!(map.get(0));
        assert!(!map.get(1));
        assert!(map.get(2));
        assert!(map.get(3));
        dbg!(map);
    }
}
