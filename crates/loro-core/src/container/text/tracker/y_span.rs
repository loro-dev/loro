use std::fmt::Display;

use crate::{
    container::text::text_content::ListSlice, id::Counter, span::IdSpan, ContentType,
    InsertContentTrait, ID,
};
use rle::{rle_tree::tree_trait::CumulateTreeTrait, HasLength, Mergable, Sliceable};

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Status {
    /// is this span from a future operation
    pub future: bool,
    pub delete_times: usize,
    pub undo_times: usize,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_activated() {
            write!(f, "Active",)
        } else {
            write!(
                f,
                "unapplied: {}, delete_times: {}, undo_times: {}",
                self.future, self.delete_times, self.undo_times
            )
        }
    }
}

impl Status {
    #[inline]
    pub fn new() -> Self {
        Status {
            future: false,
            delete_times: 0,
            undo_times: 0,
        }
    }

    #[inline]
    pub fn is_activated(&self) -> bool {
        !self.future && self.delete_times == 0 && self.undo_times == 0
    }

    /// Return whether the activation changed
    #[inline]
    pub fn apply(&mut self, change: StatusChange) -> bool {
        let activated = self.is_activated();
        match change {
            StatusChange::SetAsCurrent => self.future = false,
            StatusChange::SetAsFuture => self.future = true,
            StatusChange::Redo => self.undo_times -= 1,
            StatusChange::Undo => self.undo_times += 1,
            StatusChange::Delete => self.delete_times += 1,
            StatusChange::UndoDelete => self.delete_times -= 1,
        }

        self.is_activated() != activated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YSpan {
    pub id: ID,
    pub len: usize,
    pub status: Status,
    pub origin_left: Option<ID>,
    pub origin_right: Option<ID>,
    // TODO: remove this field when the system is stable
    pub slice: ListSlice,
}

#[derive(Clone, Copy, Debug)]
pub enum StatusChange {
    SetAsCurrent,
    SetAsFuture,
    Redo,
    Undo,
    Delete,
    UndoDelete,
}

pub(super) type YSpanTreeTrait = CumulateTreeTrait<YSpan, 4>;

impl YSpan {
    /// this is the last id of the span, which is **included** by self
    #[inline]
    pub fn last_id(&self) -> ID {
        self.id.inc(self.len as i32 - 1)
    }

    #[inline]
    pub fn can_be_origin(&self) -> bool {
        self.status.is_activated()
    }

    #[inline]
    pub fn contain_id(&self, id: ID) -> bool {
        self.id.client_id == id.client_id
            && self.id.counter <= id.counter
            && id.counter < self.id.counter + self.len as i32
    }

    #[inline]
    pub fn overlap(&self, id: IdSpan) -> bool {
        if self.id.client_id != id.client_id {
            return false;
        }

        self.id.counter < id.counter.end
            && self.id.counter + (self.len as Counter) > id.counter.min()
    }
}

impl Mergable for YSpan {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        other.id.client_id == self.id.client_id
            && self.status == other.status
            && self.id.counter + self.len as Counter == other.id.counter
            && self.origin_right == other.origin_right
            && Some(self.id.inc(self.len as Counter - 1)) == other.origin_left
            && self.slice.is_mergable(&other.slice, &())
    }

    fn merge(&mut self, other: &Self, _: &()) {
        self.origin_right = other.origin_right;
        self.len += other.len;
        self.slice.merge(&other.slice, &())
    }
}

impl Sliceable for YSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        if from == 0 && to == self.atom_len() {
            return self.clone();
        }

        let origin_left = if from == 0 {
            self.origin_left
        } else {
            Some(self.id.inc(from as i32 - 1))
        };

        // origin_right should be the same
        let origin_right = self.origin_right;
        YSpan {
            origin_left,
            origin_right,
            id: self.id.inc(from as i32),
            len: to - from,
            status: self.status.clone(),
            slice: self.slice.slice(from, to),
        }
    }
}

impl InsertContentTrait for YSpan {
    fn id(&self) -> ContentType {
        ContentType::Text
    }
}

impl HasLength for YSpan {
    #[inline]
    fn content_len(&self) -> usize {
        if self.status.is_activated() {
            self.len
        } else {
            0
        }
    }

    #[inline]
    fn atom_len(&self) -> usize {
        self.len
    }
}

#[cfg(any(test, features = "fuzzing"))]
pub mod test {
    use crate::{
        op::{InsertContent, OpContent},
        ContentType, Op, ID,
    };
    use rle::{HasLength, RleVecWithIndex};

    use super::YSpan;

    #[test]
    fn test_merge() {
        let mut vec: RleVecWithIndex<Op> = RleVecWithIndex::new();
        vec.push(Op::new(
            ID::new(0, 1),
            OpContent::Normal {
                content: InsertContent::Dyn(Box::new(YSpan {
                    origin_left: Some(ID::new(0, 0)),
                    origin_right: None,
                    id: ID::new(0, 1),
                    len: 1,
                    status: Default::default(),
                    slice: Default::default(),
                })),
            },
            5,
        ));
        vec.push(Op::new(
            ID::new(0, 2),
            OpContent::Normal {
                content: InsertContent::Dyn(Box::new(YSpan {
                    origin_left: Some(ID::new(0, 1)),
                    origin_right: None,
                    id: ID::new(0, 2),
                    len: 1,
                    status: Default::default(),
                    slice: Default::default(),
                })),
            },
            5,
        ));
        assert_eq!(vec.merged_len(), 1);
        let merged = vec.get_merged(0).unwrap();
        assert_eq!(merged.content.as_normal().unwrap().id(), ContentType::Text);
        let text_content = merged.content.as_normal().unwrap().as_dyn().unwrap();
        assert_eq!(text_content.content_len(), 2);
    }

    #[test]
    fn slice() {
        let mut vec: RleVecWithIndex<Op> = RleVecWithIndex::new();
        vec.push(Op::new(
            ID::new(0, 1),
            OpContent::Normal {
                content: InsertContent::Dyn(Box::new(YSpan {
                    origin_left: Some(ID::new(0, 0)),
                    origin_right: None,
                    id: ID::new(0, 1),
                    len: 4,
                    status: Default::default(),
                    slice: Default::default(),
                })),
            },
            5,
        ));
        vec.push(Op::new(
            ID::new(0, 2),
            OpContent::Normal {
                content: InsertContent::Dyn(Box::new(YSpan {
                    origin_left: Some(ID::new(0, 0)),
                    origin_right: Some(ID::new(0, 1)),
                    id: ID::new(0, 5),
                    len: 4,
                    status: Default::default(),
                    slice: Default::default(),
                })),
            },
            5,
        ));
        assert_eq!(vec.merged_len(), 2);
        assert_eq!(
            vec.slice_iter(2, 6)
                .map(|x| x.into_inner().content.content_len())
                .collect::<Vec<usize>>(),
            vec![2, 2]
        )
    }
}
