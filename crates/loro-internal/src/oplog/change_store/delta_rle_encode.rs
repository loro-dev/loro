use std::io::Write;

use either::Either;
#[derive(Clone, Debug)]
enum RleState<T> {
    NonRle { vec: Vec<T>, same_suffix_len: usize },
    Rle { value: T, count: usize },
}

impl<T: Copy + Eq> RleState<T> {
    pub fn new() -> Self {
        Self::NonRle {
            vec: Vec::new(),
            same_suffix_len: 1,
        }
    }

    #[must_use]
    pub fn push(&mut self, value: T, min_size_to_use_rle: usize) -> Option<RleState<T>> {
        match self {
            RleState::NonRle {
                vec,
                same_suffix_len,
            } => {
                if let Some(last) = vec.last() {
                    if *last == value {
                        *same_suffix_len += 1;
                        if *same_suffix_len >= min_size_to_use_rle {
                            let last = vec.pop().unwrap();
                            let vec = std::mem::take(vec);
                            *self = RleState::Rle {
                                value: last,
                                count: *same_suffix_len,
                            };
                            Some(RleState::NonRle {
                                vec,
                                same_suffix_len: 1,
                            })
                        } else {
                            None
                        }
                    } else {
                        vec.push(value);
                        *same_suffix_len = 1;
                        None
                    }
                } else {
                    *same_suffix_len = 1;
                    vec.push(value);
                    None
                }
            }
            RleState::Rle { value: last, count } => {
                if *last == value {
                    *count += 1;
                    None
                } else {
                    Some(std::mem::replace(
                        self,
                        RleState::NonRle {
                            vec: vec![value],
                            same_suffix_len: 1,
                        },
                    ))
                }
            }
        }
    }

    pub fn flush<W: ?Sized + Write>(
        &self,
        w: &mut W,
        mut write_t: impl FnMut(&mut W, T) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        match self {
            RleState::NonRle {
                vec,
                same_suffix_len,
            } => {
                leb128::write::signed(w, (vec.len() + same_suffix_len - 1) as i64).unwrap();
                for v in vec {
                    write_t(w, *v)?;
                }
                for _ in 1..*same_suffix_len {
                    write_t(w, *vec.last().unwrap())?;
                }
            }
            RleState::Rle { value, count } => {
                leb128::write::signed(w, -(*count as i64))?;
                write_t(w, *value)?;
            }
        }

        Ok(())
    }

    pub fn from_reader<R: ?Sized + std::io::Read>(
        r: &mut R,
        mut read_t: impl FnMut(&mut R) -> std::io::Result<T>,
    ) -> std::io::Result<Self> {
        let count = leb128::read::signed(r).unwrap();
        if count >= 0 {
            let mut vec = Vec::new();
            for _ in 0..count {
                vec.push(read_t(r)?);
            }
            Ok(RleState::NonRle {
                vec,
                same_suffix_len: 1,
            })
        } else {
            let count = -count;
            let value = read_t(r)?;
            Ok(RleState::Rle {
                value,
                count: count as usize,
            })
        }
    }
}

impl<T: Copy + Eq> Default for RleState<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct RleEncoderInner<T> {
    state: RleState<T>,
    min_size_to_use_rle: usize,
}

impl<T: Copy + Eq> RleEncoderInner<T> {
    pub fn new() -> Self {
        Self {
            state: RleState::new(),
            min_size_to_use_rle: 2,
        }
    }

    pub fn new_with_min_size_to_use_rle(min_size_to_use_rle: usize) -> Self {
        Self {
            state: RleState::new(),
            min_size_to_use_rle,
        }
    }

    pub fn push(&mut self, value: T) -> Option<RleState<T>> {
        self.state.push(value, self.min_size_to_use_rle)
    }

    pub fn take(&mut self) -> RleState<T> {
        let ans = std::mem::take(&mut self.state);
        ans
    }
}

pub struct UnsignedRleEncoder {
    v: Vec<u8>,
    last: u64,
    rle: RleEncoderInner<u64>,
    rounds: usize,
}

impl UnsignedRleEncoder {
    pub fn new(estimate_bytes: usize) -> Self {
        Self {
            v: Vec::new(),
            last: 0,
            rle: RleEncoderInner::new(),
            rounds: 0,
        }
    }

    pub fn push(&mut self, value: u64) {
        match self.rle.push(value) {
            None => {}
            Some(to_flush) => {
                self.flush(to_flush);
            }
        }
    }

    fn flush(&mut self, to_flush: RleState<u64>) {
        to_flush
            .flush(&mut self.v, |w, v| {
                leb128::write::unsigned(w, v).map(|_| ())
            })
            .unwrap();
        self.rounds += 1;
    }

    pub fn finish(mut self) -> (Vec<u8>, usize) {
        let to_flush = self.rle.take();
        self.flush(to_flush);
        let v = self.v;
        (v, self.rounds)
    }
}

pub struct UnsignedRleDecoder<'a> {
    v: &'a [u8],
    nth_round: usize,
    current_rle: Either<i64, (u64, i64)>, // (value, remaining count)
}

impl<'a> UnsignedRleDecoder<'a> {
    pub fn new(v: &'a [u8], round: usize) -> Self {
        Self {
            v,
            nth_round: round,
            current_rle: Either::Left(0),
        }
    }

    pub fn next(&mut self) -> Option<u64> {
        match &mut self.current_rle {
            Either::Left(count) => {
                if *count > 0 {
                    *count -= 1;
                    let value = leb128::read::unsigned(&mut self.v).unwrap();
                    return Some(value);
                }
            }
            Either::Right((value, count)) => {
                if *count > 0 {
                    *count -= 1;
                    return Some(*value);
                }
            }
        }

        if self.nth_round == 0 {
            return None;
        }

        self.nth_round -= 1;
        let len = leb128::read::signed(&mut self.v).unwrap();
        if len < 0 {
            // Read the RLE value and count
            let value = leb128::read::unsigned(&mut self.v).unwrap();
            self.current_rle = Either::Right((value, -len));
            self.next()
        } else {
            // Read the non-RLE value
            self.current_rle = Either::Left(len);
            self.next()
        }
    }
}

pub struct SignedDeltaEncoder {
    v: Vec<u8>,
    last: i64,
    round: usize,
}

impl SignedDeltaEncoder {
    pub fn new(estimate_bytes: usize) -> Self {
        Self {
            v: Vec::new(),
            last: 0,
            round: 0,
        }
    }

    pub fn push(&mut self, value: i64) {
        let delta = value - self.last;
        self.last = value;
        leb128::write::signed(&mut self.v, delta).unwrap();
        self.round += 1;
    }

    pub fn finish(self) -> (Vec<u8>, usize) {
        let v = self.v;
        (v, self.round)
    }
}

pub struct SignedDeltaDecoder<'a> {
    v: &'a [u8],
    count: usize,
    last: i64,
}

impl<'a> SignedDeltaDecoder<'a> {
    pub fn new(v: &'a [u8], count: usize) -> Self {
        Self { v, count, last: 0 }
    }

    pub fn next(&mut self) -> Option<i64> {
        if self.count == 0 {
            return None;
        }

        match leb128::read::signed(&mut self.v) {
            Ok(delta) => {
                self.last += delta;
                self.count -= 1;
                Some(self.last)
            }
            Err(_) => None,
        }
    }
}

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

pub struct SignedDeltaRleEncoder {
    v: Vec<u8>,
    last: i64,
    count: usize,
    rle: RleEncoderInner<i64>,
}

impl SignedDeltaRleEncoder {
    pub fn new(estimate_bytes: usize) -> Self {
        let v = Vec::with_capacity(estimate_bytes);
        Self {
            v,
            last: 0,
            count: 0,
            rle: RleEncoderInner::new(),
        }
    }

    pub fn push(&mut self, value: i64) {
        let delta = value - self.last;
        match self.rle.push(delta) {
            None => {}
            Some(to_flush) => {
                self.flush(to_flush);
            }
        }
    }

    fn flush(&mut self, to_flush: RleState<i64>) {
        to_flush
            .flush(&mut self.v, |w, v| {
                leb128::write::signed(w, v)?;
                Ok(())
            })
            .unwrap();
        self.count += 1;
    }

    pub fn finish(mut self) -> (Vec<u8>, usize) {
        let to_flush = self.rle.take();
        self.flush(to_flush);
        (self.v, self.count)
    }
}

pub struct SignedDeltaRleDecoder<'a> {
    v: &'a [u8],
    count: usize,
    state: Either<usize, (i64, usize)>,
}

impl<'a> SignedDeltaRleDecoder<'a> {
    pub fn new(v: &'a [u8], count: usize) -> Self {
        Self {
            v,
            count,
            state: Either::Left(0),
        }
    }

    pub fn rest(mut self) -> &'a [u8] {
        while self.next().is_some() {}

        self.v
    }
}

impl<'a> Iterator for SignedDeltaRleDecoder<'a> {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            Either::Left(len) => {
                if *len > 0 {
                    *len -= 1;
                    let next = leb128::read::signed(&mut self.v).unwrap();
                    return Some(next);
                }
            }
            Either::Right((next, count)) => {
                if *count > 0 {
                    *count -= 1;
                    return Some(*next);
                }
            }
        }

        if self.count == 0 {
            return None;
        }

        self.count -= 1;
        let len = leb128::read::signed(&mut self.v).unwrap();
        if len < 0 {
            // RLE
            let last = leb128::read::signed(&mut self.v).unwrap();
            self.state = Either::Right((last, (-len) as usize));
            self.next()
        } else {
            // non-RLE
            self.state = Either::Left(len as usize);
            self.next()
        }
    }
}

pub struct UnsignedDeltaRleEncoder {
    v: Vec<u8>,
    last: u64,
    count: usize,
    rle: RleEncoderInner<u64>,
}

impl UnsignedDeltaRleEncoder {
    pub fn new(estimate_bytes: usize) -> Self {
        let v = Vec::with_capacity(estimate_bytes);
        Self {
            v,
            last: 0,
            count: 0,
            rle: RleEncoderInner::new(),
        }
    }

    pub fn push(&mut self, value: u64) {
        let delta = value - self.last;
        match self.rle.push(delta) {
            None => {}
            Some(to_flush) => {
                self.flush(to_flush);
            }
        }
    }

    fn flush(&mut self, to_flush: RleState<u64>) {
        to_flush
            .flush(&mut self.v, |w, v| {
                leb128::write::unsigned(w, v)?;
                Ok(())
            })
            .unwrap();
        self.count += 1;
    }

    pub fn finish(mut self) -> (Vec<u8>, usize) {
        let to_flush = self.rle.take();
        self.flush(to_flush);
        (self.v, self.count)
    }
}

pub struct UnsignedDeltaRleDecoder<'a> {
    v: &'a [u8],
    count: usize,
    state: Either<usize, (u64, usize)>,
}

impl<'a> UnsignedDeltaRleDecoder<'a> {
    pub fn new(v: &'a [u8], count: usize) -> Self {
        Self {
            v,
            count,
            state: Either::Left(0),
        }
    }

    pub fn rest(mut self) -> &'a [u8] {
        while self.next().is_some() {}
        self.v
    }
}

impl<'a> Iterator for UnsignedDeltaRleDecoder<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            Either::Left(len) => {
                if *len > 0 {
                    *len -= 1;
                    let next = leb128::read::unsigned(&mut self.v).unwrap();
                    return Some(next);
                }
            }
            Either::Right((next, count)) => {
                if *count > 0 {
                    *count -= 1;
                    return Some(*next);
                }
            }
        }

        if self.count == 0 {
            return None;
        }

        self.count -= 1;
        let len = leb128::read::signed(&mut self.v).unwrap();
        if len < 0 {
            // RLE
            let last = leb128::read::unsigned(&mut self.v).unwrap();
            self.state = Either::Right((last, (-len) as usize));
            self.next()
        } else {
            // non-RLE
            self.state = Either::Left(len as usize);
            self.next()
        }
    }
}

pub struct BoolRleEncoder {
    v: Vec<u8>,
    count: usize,
    rle: RleEncoderInner<bool>,
}

impl BoolRleEncoder {
    pub fn new() -> Self {
        BoolRleEncoder {
            v: Vec::new(),
            count: 0,
            rle: RleEncoderInner::new_with_min_size_to_use_rle(8),
        }
    }

    pub fn push(&mut self, value: bool) {
        if let Some(to_flush) = self.rle.push(value) {
            self.flush(to_flush);
        }
    }

    pub fn finish(self) -> (Vec<u8>, usize) {
        (self.v, self.count)
    }

    fn flush(&mut self, to_flush: RleState<bool>) {
        to_flush
            .flush(&mut self.v, |w, v| {
                w.write_all(&[if v { 1 } else { 0 }])?;
                Ok(())
            })
            .unwrap();
        self.count += 1;
    }
}

pub struct BoolRleDecoder<'a> {
    v: &'a [u8],
    pos: usize,
    last: bool,
    count: u16,
}

impl<'a> BoolRleDecoder<'a> {
    pub fn new(v: &'a [u8]) -> Self {
        BoolRleDecoder {
            v,
            pos: 0,
            last: false,
            count: 0,
        }
    }
}

impl<'a> Iterator for BoolRleDecoder<'a> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.count > 0 {
            self.count -= 1;
            return Some(self.last);
        }

        if self.pos >= self.v.len() {
            return None;
        }

        self.last = self.v[self.pos] != 0;
        self.count = u16::from_le_bytes([self.v[self.pos + 1], self.v[self.pos + 2]]);
        self.pos += 3;

        self.next()
    }
}
