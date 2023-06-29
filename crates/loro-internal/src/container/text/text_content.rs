use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};
use smallvec::{smallvec, SmallVec};

use crate::{
    delta::{DeltaItem, DeltaValue},
    smstring::SmString,
    LoroValue,
};

use super::string_pool::PoolString;

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(PartialEq, Debug, EnumAsInner, Clone, Serialize, Deserialize)]
pub enum ListSlice {
    // TODO: use Box<[LoroValue]> ?
    RawData(Vec<LoroValue>),
    RawStr(SmString),
    Unknown(usize),
}

#[repr(transparent)]
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct SliceRange(pub Range<u32>);

const UNKNOWN_START: u32 = u32::MAX / 2;
impl SliceRange {
    #[inline(always)]
    pub fn is_unknown(&self) -> bool {
        self.0.start == UNKNOWN_START
    }

    pub fn new_unknown(size: u32) -> Self {
        Self(UNKNOWN_START..UNKNOWN_START + size)
    }

    pub fn from_pool_string(p: &PoolString) -> Self {
        match &p.slice {
            Some(x) => Self(x.start() as u32..x.end() as u32),
            None => Self::new_unknown(p.unknown_len),
        }
    }
}

impl Default for ListSlice {
    fn default() -> Self {
        ListSlice::Unknown(0)
    }
}

impl From<Range<u32>> for SliceRange {
    fn from(a: Range<u32>) -> Self {
        SliceRange(a)
    }
}

impl HasLength for SliceRange {
    fn content_len(&self) -> usize {
        self.0.len()
    }
}

impl Sliceable for SliceRange {
    fn slice(&self, from: usize, to: usize) -> Self {
        if self.is_unknown() {
            Self::new_unknown((to - from) as u32)
        } else {
            SliceRange(self.0.start + from as u32..self.0.start + to as u32)
        }
    }
}

impl Mergable for SliceRange {
    fn merge(&mut self, other: &Self, _: &()) {
        if self.is_unknown() {
            self.0.end += other.0.end - other.0.start;
        } else {
            self.0.end = other.0.end;
        }
    }

    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        (self.is_unknown() && other.is_unknown()) || self.0.end == other.0.start
    }
}

impl ListSlice {
    #[inline(always)]
    pub fn unknown_range(len: usize) -> SliceRange {
        let start = UNKNOWN_START;
        let end = len as u32 + UNKNOWN_START;
        SliceRange(start..end)
    }

    #[inline(always)]
    pub fn is_unknown(range: &SliceRange) -> bool {
        range.is_unknown()
    }
}

impl HasLength for ListSlice {
    fn content_len(&self) -> usize {
        match self {
            ListSlice::RawStr(s) => s.len(),
            ListSlice::Unknown(x) => *x,
            ListSlice::RawData(x) => x.len(),
        }
    }
}

impl Sliceable for ListSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::RawStr(s) => ListSlice::RawStr(s[from..to].into()),
            ListSlice::Unknown(_) => ListSlice::Unknown(to - from),
            ListSlice::RawData(x) => ListSlice::RawData(x[from..to].to_vec()),
        }
    }
}

impl Mergable for ListSlice {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        match (self, other) {
            (ListSlice::Unknown(_), ListSlice::Unknown(_)) => true,
            (ListSlice::RawStr(a), ListSlice::RawStr(b)) => a.is_mergable(b, &()),
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &()) {
        match (self, other) {
            (ListSlice::Unknown(x), ListSlice::Unknown(y)) => {
                *x += y;
            }
            (ListSlice::RawStr(a), ListSlice::RawStr(b)) => a.merge(b, &()),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SliceRanges(pub SmallVec<[SliceRange; 2]>);

impl From<SliceRange> for SliceRanges {
    fn from(value: SliceRange) -> Self {
        Self(smallvec![value])
    }
}

impl DeltaValue for SliceRanges {
    fn value_extend(&mut self, other: Self) {
        self.0.extend(other.0.into_iter());
    }

    fn take(&mut self, target_len: usize) -> Self {
        let mut ret = SmallVec::new();
        let mut cur_len = 0;
        while cur_len < target_len {
            let range = self.0.pop().unwrap();
            let range_len = range.content_len();
            if cur_len + range_len <= target_len {
                ret.push(range);
                cur_len += range_len;
            } else {
                let new_range = range.slice(0, target_len - cur_len);
                ret.push(new_range);
                self.0.push(range.slice(target_len - cur_len, range_len));
                cur_len = target_len;
            }
        }
        SliceRanges(ret)
    }

    fn length(&self) -> usize {
        self.0.iter().fold(0, |acc, x| acc + x.atom_len())
    }
}

#[cfg(test)]
mod test {
    use crate::LoroValue;

    use super::ListSlice;

    #[test]
    fn fix_fields_order() {
        let list_slice = vec![
            ListSlice::RawData(vec![LoroValue::Bool(true)]),
            ListSlice::RawStr("".into()),
            ListSlice::Unknown(0),
        ];
        let list_slice_buf = vec![3, 0, 1, 1, 1, 1, 0, 2, 0];
        assert_eq!(
            postcard::from_bytes::<Vec<ListSlice>>(&list_slice_buf).unwrap(),
            list_slice
        );
    }
}
