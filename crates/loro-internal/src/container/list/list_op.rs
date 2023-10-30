use std::ops::Range;

use append_only_bytes::BytesSlice;
use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::{
    container::richtext::TextStyleInfoFlag,
    op::{ListSlice, SliceRange},
    utils::string_slice::unicode_range_to_byte_range,
    InternalString,
};

/// `len` and `pos` is measured in unicode char for text.
// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(EnumAsInner, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListOp<'a> {
    Insert {
        slice: ListSlice<'a>,
        pos: usize,
    },
    Delete(DeleteSpan),
    /// StyleStart and StyleEnd must be paired because the end of a style must take an OpID position.
    StyleStart {
        start: u32,
        end: u32,
        key: InternalString,
        info: TextStyleInfoFlag,
    },
    StyleEnd,
}

#[derive(EnumAsInner, Debug, Clone)]
pub enum InnerListOp {
    // Note: len may not equal to slice.len() because for text len is unicode len while the slice
    // is utf8 bytes.
    Insert {
        slice: SliceRange,
        pos: usize,
    },
    InsertText {
        slice: BytesSlice,
        unicode_start: u32,
        unicode_len: u32,
        pos: u32,
    },
    Delete(DeleteSpan),
    /// StyleStart and StyleEnd must be paired.
    StyleStart {
        start: u32,
        end: u32,
        key: InternalString,
        info: TextStyleInfoFlag,
    },
    StyleEnd,
}

impl<'a> ListOp<'a> {
    pub fn new_del(pos: usize, len: usize) -> Self {
        assert!(len != 0);
        Self::Delete(DeleteSpan {
            pos: pos as isize,
            signed_len: len as isize,
        })
    }
}

impl InnerListOp {
    pub fn new_del(pos: usize, len: isize) -> Self {
        assert!(len != 0);
        Self::Delete(DeleteSpan {
            pos: pos as isize,
            signed_len: len,
        })
    }

    pub fn new_insert(slice: Range<u32>, pos: usize) -> Self {
        Self::Insert {
            slice: SliceRange(slice),
            pos,
        }
    }
}

impl HasLength for DeleteSpan {
    fn content_len(&self) -> usize {
        self.signed_len.unsigned_abs()
    }
}

/// `len` can be negative so that we can merge text deletions efficiently.
/// It looks like [crate::span::CounterSpan], but how should they merge ([Mergable] impl) and slice ([Sliceable] impl) are very different
///
/// len cannot be zero;
///
/// pos: 5, len: -3 eq a range of (2, 5]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// Note: It will be encoded into binary format, so the order of its fields should not be changed.
pub struct DeleteSpan {
    pub pos: isize,
    pub signed_len: isize,
}

impl DeleteSpan {
    pub fn new(pos: isize, len: isize) -> Self {
        debug_assert!(len != 0);
        Self {
            pos,
            signed_len: len,
        }
    }

    #[inline(always)]
    pub fn start(&self) -> isize {
        if self.signed_len > 0 {
            self.pos
        } else {
            self.pos + 1 + self.signed_len
        }
    }

    #[inline(always)]
    pub fn last(&self) -> isize {
        if self.signed_len > 0 {
            self.pos + self.signed_len - 1
        } else {
            self.pos
        }
    }

    #[inline(always)]
    pub fn end(&self) -> isize {
        if self.signed_len > 0 {
            self.pos + self.signed_len
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
        self.signed_len.abs() == 1
    }

    #[inline(always)]
    pub fn is_reversed(&self) -> bool {
        self.signed_len < 0
    }

    #[inline(always)]
    pub fn direction(&self) -> isize {
        if self.signed_len > 0 {
            1
        } else {
            -1
        }
    }

    #[inline(always)]
    pub fn next_pos(&self) -> isize {
        if self.signed_len > 0 {
            self.start()
        } else {
            self.start() - 1
        }
    }

    #[inline(always)]
    pub fn prev_pos(&self) -> isize {
        if self.signed_len > 0 {
            self.pos
        } else {
            self.end()
        }
    }

    pub fn len(&self) -> usize {
        self.signed_len.unsigned_abs()
    }
}

impl Mergable for DeleteSpan {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        // merge continuous deletions:
        // note that the previous deletions will affect the position of the later deletions
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
                    self.signed_len = 2;
                } else if self.pos == other.pos + 1 {
                    self.signed_len = -2;
                } else {
                    unreachable!()
                }
            }
            (true, false) => {
                assert!(self.pos == other.prev_pos());
                self.signed_len = other.signed_len + other.direction();
            }
            (false, true) => {
                assert!(self.next_pos() == other.pos);
                self.signed_len += self.direction();
            }
            (false, false) => {
                assert!(self.next_pos() == other.pos && self.direction() == other.direction());
                self.signed_len += other.signed_len;
            }
        }
    }
}

impl Sliceable for DeleteSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        if self.signed_len > 0 {
            Self::new(self.pos, to as isize - from as isize)
        } else {
            Self::new(self.pos - from as isize, from as isize - to as isize)
        }
    }
}

impl<'a> Mergable for ListOp<'a> {
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
            ListOp::StyleStart { .. } | ListOp::StyleEnd { .. } => false,
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
            ListOp::StyleStart { .. } | ListOp::StyleEnd { .. } => unreachable!(),
        }
    }
}

impl<'a> HasLength for ListOp<'a> {
    fn content_len(&self) -> usize {
        match self {
            ListOp::Insert { slice, .. } => slice.content_len(),
            ListOp::Delete(span) => span.atom_len(),
            ListOp::StyleStart { .. } | ListOp::StyleEnd { .. } => 1,
        }
    }
}

impl<'a> Sliceable for ListOp<'a> {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListOp::Insert { slice, pos } => ListOp::Insert {
                slice: slice.slice(from, to),
                pos: *pos + from,
            },
            ListOp::Delete(span) => ListOp::Delete(span.slice(from, to)),
            a @ (ListOp::StyleStart { .. } | ListOp::StyleEnd { .. }) => a.clone(),
        }
    }
}

impl Mergable for InnerListOp {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (
                InnerListOp::Insert { pos, slice, .. },
                InnerListOp::Insert {
                    pos: other_pos,
                    slice: other_slice,
                    ..
                },
            ) => pos + slice.content_len() == *other_pos && slice.is_mergable(other_slice, &()),
            (InnerListOp::Delete(span), InnerListOp::Delete(other_span)) => {
                span.is_mergable(other_span, &())
            }
            (
                InnerListOp::InsertText {
                    unicode_start,
                    slice,
                    pos,
                    unicode_len: len,
                },
                InnerListOp::InsertText {
                    slice: other_slice,
                    pos: other_pos,
                    unicode_start: other_unicode_start,
                    unicode_len: _,
                },
            ) => {
                pos + len == *other_pos
                    && slice.can_merge(other_slice)
                    && unicode_start + len == *other_unicode_start
            }
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match (self, other) {
            (
                InnerListOp::Insert { slice, .. },
                InnerListOp::Insert {
                    slice: other_slice, ..
                },
            ) => {
                slice.merge(other_slice, &());
            }
            (InnerListOp::Delete(span), InnerListOp::Delete(other_span)) => {
                span.merge(other_span, &())
            }
            (
                InnerListOp::InsertText {
                    slice,
                    unicode_len: len,
                    ..
                },
                InnerListOp::InsertText {
                    slice: other_slice,
                    unicode_len: other_len,
                    ..
                },
            ) => {
                slice.merge(other_slice, &());
                *len += *other_len;
            }
            _ => unreachable!(),
        }
    }
}

impl HasLength for InnerListOp {
    fn content_len(&self) -> usize {
        match self {
            InnerListOp::Insert { slice, .. } => slice.content_len(),
            InnerListOp::InsertText {
                unicode_len: len, ..
            } => *len as usize,
            InnerListOp::Delete(span) => span.atom_len(),
            InnerListOp::StyleStart { .. } | InnerListOp::StyleEnd { .. } => 1,
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
            InnerListOp::InsertText {
                slice,
                unicode_start,
                unicode_len: _,
                pos,
            } => InnerListOp::InsertText {
                slice: {
                    let (a, b) = unicode_range_to_byte_range(slice, from, to);
                    slice.slice(a, b)
                },
                unicode_start: *unicode_start + from as u32,
                unicode_len: (to - from) as u32,
                pos: *pos + from as u32,
            },
            InnerListOp::Delete(span) => InnerListOp::Delete(span.slice(from, to)),
            InnerListOp::StyleStart { .. } | InnerListOp::StyleEnd { .. } => self.clone(),
        }
    }
}

#[cfg(all(test, feature = "test_utils"))]
mod test {
    use rle::{Mergable, Sliceable};

    use crate::op::ListSlice;

    use super::{DeleteSpan, ListOp};

    #[test]
    fn fix_fields_order() {
        let list_op = vec![
            ListOp::Insert {
                pos: 0,
                slice: ListSlice::from_borrowed_str(""),
            },
            ListOp::Delete(DeleteSpan::new(0, 3)),
        ];
        let actual = postcard::to_allocvec(&list_op).unwrap();
        let list_op_buf = vec![2, 0, 1, 0, 0, 0, 1, 0, 6];
        assert_eq!(&actual, &list_op_buf);
        assert_eq!(
            postcard::from_bytes::<Vec<ListOp>>(&list_op_buf).unwrap(),
            list_op
        );

        let delete_span = DeleteSpan {
            pos: 0,
            signed_len: 3,
        };
        let delete_span_buf = vec![0, 6];
        assert_eq!(
            postcard::from_bytes::<DeleteSpan>(&delete_span_buf).unwrap(),
            delete_span
        );
    }

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
