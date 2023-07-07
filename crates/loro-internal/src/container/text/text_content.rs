use std::{borrow::Cow, ops::Range};

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{ser::SerializeSeq, Deserialize, Serialize};
use smallvec::{smallvec, SmallVec};

use crate::{delta::DeltaValue, LoroValue};

use super::string_pool::PoolString;

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(PartialEq, Debug, EnumAsInner, Clone, Serialize, Deserialize)]
pub enum ListSlice<'a> {
    RawData(Cow<'a, [LoroValue]>),
    RawStr(Cow<'a, str>),
    Unknown(usize),
}

#[repr(transparent)]
#[derive(PartialEq, Eq, Debug, Clone, Serialize)]
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

impl<'a> Default for ListSlice<'a> {
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

impl<'a> ListSlice<'a> {
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

    pub fn to_static(&self) -> ListSlice<'static> {
        match self {
            ListSlice::RawData(x) => ListSlice::RawData(Cow::Owned(x.to_vec())),
            ListSlice::RawStr(x) => ListSlice::RawStr(Cow::Owned(x.to_string())),
            ListSlice::Unknown(x) => ListSlice::Unknown(*x),
        }
    }
}

impl<'a> HasLength for ListSlice<'a> {
    fn content_len(&self) -> usize {
        match self {
            ListSlice::RawStr(s) => s.len(),
            ListSlice::Unknown(x) => *x,
            ListSlice::RawData(x) => x.len(),
        }
    }
}

impl<'a> Sliceable for ListSlice<'a> {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::RawStr(s) => ListSlice::RawStr(Cow::Owned(s[from..to].into())),
            ListSlice::Unknown(_) => ListSlice::Unknown(to - from),
            ListSlice::RawData(x) => match x {
                Cow::Borrowed(x) => ListSlice::RawData(Cow::Borrowed(&x[from..to])),
                Cow::Owned(x) => ListSlice::RawData(Cow::Owned(x[from..to].into())),
            },
        }
    }
}

impl<'a> Mergable for ListSlice<'a> {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        match (self, other) {
            (ListSlice::Unknown(_), ListSlice::Unknown(_)) => true,
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &()) {
        match (self, other) {
            (ListSlice::Unknown(x), ListSlice::Unknown(y)) => {
                *x += y;
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SliceRanges(pub SmallVec<[SliceRange; 2]>);

impl Serialize for SliceRanges {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0.iter() {
            s.serialize_element(item);
        }
        s.end()
    }
}

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
            ListSlice::RawData(vec![LoroValue::Bool(true)].into()),
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
