pub struct UnsignedDeltaEncoder {
    v: Vec<u8>,
    last: u64,
    count: usize,
}

impl UnsignedDeltaEncoder {
    pub fn new(estimate_bytes: usize) -> Self {
        Self {
            v: Vec::with_capacity(estimate_bytes),
            last: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, value: u64) {
        let delta = value - self.last;
        self.last = value;
        leb128::write::unsigned(&mut self.v, delta).unwrap();
        self.count += 1;
    }

    pub fn finish(self) -> (Vec<u8>, usize) {
        (self.v, self.count)
    }
}

pub struct UnsignedDeltaDecoder<'a> {
    v: &'a [u8],
    count: usize,
    last: u64,
}

impl<'a> UnsignedDeltaDecoder<'a> {
    pub fn new(v: &'a [u8], count: usize) -> Self {
        Self { v, count, last: 0 }
    }

    pub fn rest(mut self) -> &'a [u8] {
        while self.next().is_some() {}
        self.v
    }
}

impl<'a> Iterator for UnsignedDeltaDecoder<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.count == 0 {
            return None;
        }

        self.count -= 1;
        let delta = leb128::read::unsigned(&mut self.v).unwrap();
        self.last += delta;
        Some(self.last)
    }
}
