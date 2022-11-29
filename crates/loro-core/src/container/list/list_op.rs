use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::container::text::text_content::{ListSlice, SliceRange};

#[derive(EnumAsInner, Debug, Clone, Serialize, Deserialize)]
pub enum ListOp {
    Insert { slice: ListSlice, pos: usize },
    Delete(DeleteSpan),
}

#[derive(EnumAsInner, Debug, Clone)]
pub enum InnerListOp {
    Insert { slice: SliceRange, pos: usize },
    Delete(DeleteSpan),
}

/// `len` can be negative so that we can merge text deletions efficiently.
/// It looks like [crate::span::CounterSpan], but how should they merge ([Mergable] impl) and slice ([Sliceable] impl) are very different
///
/// len cannot be zero;
///
/// pos: 5, len: -3 eq a range of (2, 5]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteSpan {
    pub pos: isize,
    pub len: isize,
}

impl ListOp {
    pub fn new_del(pos: usize, len: usize) -> Self {
        assert!(len != 0);
        Self::Delete(DeleteSpan {
            pos: pos as isize,
            len: len as isize,
        })
    }
}

impl InnerListOp {
    pub fn new_del(pos: usize, len: usize) -> Self {
        assert!(len != 0);
        Self::Delete(DeleteSpan {
            pos: pos as isize,
            len: len as isize,
        })
    }
}

impl HasLength for DeleteSpan {
    fn content_len(&self) -> usize {
        self.len.unsigned_abs()
    }
}

impl DeleteSpan {
    pub fn new(pos: isize, len: isize) -> Self {
        debug_assert!(len != 0);
        Self { pos, len }
    }

    #[inline(always)]
    pub fn start(&self) -> isize {
        if self.len > 0 {
            self.pos
        } else {
            self.pos + 1 + self.len
        }
    }

    #[inline(always)]
    pub fn last(&self) -> isize {
        if self.len > 0 {
            self.pos + self.len - 1
        } else {
            self.pos
        }
    }

    #[inline(always)]
    pub fn end(&self) -> isize {
        if self.len > 0 {
            self.pos + self.len
        } else {
            self.pos + 1
        }
    }

    #[inline(always)]
    pub fn to_range(self) -> Range<isize> {
        self.start()..self.end()
    }

    #[inline(always)]
    pub fn bidirectional(&self) -> bool {
        self.len.abs() == 1
    }

    #[inline(always)]
    pub fn is_reversed(&self) -> bool {
        self.len < 0
    }

    #[inline(always)]
    pub fn direction(&self) -> isize {
        if self.len > 0 {
            1
        } else {
            -1
        }
    }

    #[inline(always)]
    pub fn next_pos(&self) -> isize {
        if self.len > 0 {
            self.start()
        } else {
            self.start() - 1
        }
    }

    #[inline(always)]
    pub fn prev_pos(&self) -> isize {
        if self.len > 0 {
            self.pos
        } else {
            self.end()
        }
    }
}

impl Mergable for DeleteSpan {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self.bidirectional(), other.bidirectional()) {
            (true, true) => self.pos == other.pos || self.pos == other.pos + 1,
            (true, false) => self.pos == other.prev_pos(),
            (false, true) => self.next_pos() == other.pos,
            (false, false) => self.next_pos() == other.pos && self.direction() == other.direction(),
        }
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match (self.bidirectional(), other.bidirectional()) {
            (true, true) => {
                if self.pos == other.pos {
                    self.len = 2;
                } else if self.pos == other.pos + 1 {
                    self.len = -2;
                } else {
                    unreachable!()
                }
            }
            (true, false) => {
                assert!(self.pos == other.prev_pos());
                self.len = other.len + other.direction();
            }
            (false, true) => {
                assert!(self.next_pos() == other.pos);
                self.len += self.direction();
            }
            (false, false) => {
                assert!(self.next_pos() == other.pos && self.direction() == other.direction());
                self.len += other.len;
            }
        }
    }
}

impl Sliceable for DeleteSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        if self.len > 0 {
            Self::new(self.pos, to as isize - from as isize)
        } else {
            Self::new(self.pos - from as isize, from as isize - to as isize)
        }
    }
}

impl Mergable for ListOp {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match self {
            ListOp::Insert { pos, slice } => match _other {
                ListOp::Insert {
                    pos: other_pos,
                    slice: other_slice,
                } => pos + slice.content_len() == *other_pos && slice.is_mergable(other_slice, &()),
                _ => false,
            },
            &ListOp::Delete(span) => match _other {
                ListOp::Delete(other_span) => span.is_mergable(other_span, &()),
                _ => false,
            },
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            ListOp::Insert { slice, .. } => match _other {
                ListOp::Insert {
                    slice: other_slice, ..
                } => {
                    slice.merge(other_slice, &());
                }
                _ => unreachable!(),
            },
            ListOp::Delete(span) => match _other {
                ListOp::Delete(other_span) => span.merge(other_span, &()),
                _ => unreachable!(),
            },
        }
    }
}

impl HasLength for ListOp {
    fn content_len(&self) -> usize {
        match self {
            ListOp::Insert { slice, .. } => slice.content_len(),
            ListOp::Delete(span) => span.atom_len(),
        }
    }
}

impl Sliceable for ListOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListOp::Insert { slice, pos } => ListOp::Insert {
                slice: slice.slice(from, to),
                pos: *pos + from,
            },
            ListOp::Delete(span) => ListOp::Delete(span.slice(from, to)),
        }
    }
}

impl Mergable for InnerListOp {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match self {
            InnerListOp::Insert { pos, slice } => match _other {
                InnerListOp::Insert {
                    pos: other_pos,
                    slice: other_slice,
                } => pos + slice.content_len() == *other_pos && slice.is_mergable(other_slice, &()),
                _ => false,
            },
            &InnerListOp::Delete(span) => match _other {
                InnerListOp::Delete(other_span) => span.is_mergable(other_span, &()),
                _ => false,
            },
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            InnerListOp::Insert { slice, .. } => match _other {
                InnerListOp::Insert {
                    slice: other_slice, ..
                } => {
                    slice.merge(other_slice, &());
                }
                _ => unreachable!(),
            },
            InnerListOp::Delete(span) => match _other {
                InnerListOp::Delete(other_span) => span.merge(other_span, &()),
                _ => unreachable!(),
            },
        }
    }
}

impl HasLength for InnerListOp {
    fn content_len(&self) -> usize {
        match self {
            InnerListOp::Insert { slice, .. } => slice.content_len(),
            InnerListOp::Delete(span) => span.atom_len(),
        }
    }
}

impl Sliceable for InnerListOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            InnerListOp::Insert { slice, pos } => InnerListOp::Insert {
                slice: slice.slice(from, to),
                pos: *pos + from,
            },
            InnerListOp::Delete(span) => InnerListOp::Delete(span.slice(from, to)),
        }
    }
}

#[cfg(test)]
mod test {
    use rle::{Mergable, Sliceable};

    use super::DeleteSpan;

    #[test]
    fn test_del_span_merge_slice() {
        let a = DeleteSpan::new(0, 100);
        let mut b = a.slice(0, 1);
        let c = a.slice(1, 100);
        b.merge(&c, &());
        assert_eq!(b, a);

        // reverse
        let a = DeleteSpan::new(99, -100);
        let mut b = a.slice(0, 1);
        let c = a.slice(1, 100);
        b.merge(&c, &());
        assert_eq!(b, a);

        // merge bidirectional
        let mut a = DeleteSpan::new(1, -1);
        let b = DeleteSpan::new(1, -1);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, DeleteSpan::new(1, 2));

        let mut a = DeleteSpan::new(1, -1);
        let b = DeleteSpan::new(0, -1);
        assert_eq!(b.to_range(), 0..1);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, DeleteSpan::new(1, -2));

        // not merging
        let a = DeleteSpan::new(4, 1);
        let b = DeleteSpan::new(5, 2);
        assert!(!a.is_mergable(&b, &()));

        // next/prev span
        let a = DeleteSpan::new(6, -2);
        assert_eq!(a.next_pos(), 4);
        assert_eq!(a.prev_pos(), 7);
        let a = DeleteSpan::new(6, 2);
        assert_eq!(a.next_pos(), 6);
        assert_eq!(a.prev_pos(), 6);
        assert!(a.slice(0, 1).is_mergable(&a.slice(1, 2), &()));

        // neg merge
        let mut a = DeleteSpan::new(1, 1);
        let b = DeleteSpan::new(0, 1);
        a.merge(&b, &());
        assert_eq!(a, DeleteSpan::new(1, -2));
        assert_eq!(a.slice(0, 1), DeleteSpan::new(1, -1));
        assert_eq!(a.slice(1, 2), DeleteSpan::new(0, -1));
        assert_eq!(a.slice(0, 1).to_range(), 1..2);
        assert_eq!(a.slice(1, 2).to_range(), 0..1);
    }
}
