use std::{ops::Range, sync::Arc};

use jumprope::JumpRope;

use crate::{
    container::text::text_content::ListSlice,
    delta::DeltaItem,
    event::Diff,
    op::{RawOp, RawOpContent},
    refactor::arena::SharedArena,
    LoroValue,
};

use super::ContainerState;

#[derive(Default)]
pub struct Text {
    pub(crate) rope: JumpRope,
    in_txn: bool,
    deleted_bytes: Vec<u8>,
    undo_stack: Vec<UndoItem>,
}

impl Clone for Text {
    fn clone(&self) -> Self {
        Self {
            rope: self.rope.clone(),
            in_txn: false,
            deleted_bytes: Default::default(),
            undo_stack: Default::default(),
        }
    }
}

enum UndoItem {
    Insert {
        index: u32,
        len: u32,
    },
    Delete {
        index: u32,
        byte_offset: u32,
        len: u32,
    },
}

impl ContainerState for Text {
    fn apply_diff(&mut self, diff: &Diff, arena: &SharedArena) {
        match diff {
            Diff::SeqRaw(delta) => {
                let mut index = 0;
                for span in delta.iter() {
                    match span {
                        DeltaItem::Retain { len, meta: _ } => {
                            index += len;
                        }
                        DeltaItem::Insert { value, .. } => {
                            for value in value.0.iter() {
                                let s =
                                    arena.slice_bytes(value.0.start as usize..value.0.end as usize);
                                self.insert(index, std::str::from_utf8(&s).unwrap());
                                index += s.len();
                            }
                        }
                        DeltaItem::Delete { len, .. } => {
                            self.delete(index..index + len);
                        }
                    }
                }
            }
            Diff::Text(delta) => {
                let mut index = 0;
                for span in delta.iter() {
                    match span {
                        DeltaItem::Retain { len, meta: _ } => {
                            index += len;
                        }
                        DeltaItem::Insert { value, .. } => {
                            self.insert(index, value);
                            index += value.len();
                        }
                        DeltaItem::Delete { len, .. } => {
                            self.delete(index..index + len);
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn apply_op(&mut self, op: RawOp) {
        match op.content {
            RawOpContent::List(list) => match list {
                crate::container::list::list_op::ListOp::Insert { slice, pos } => match slice {
                    ListSlice::RawStr(s) => {
                        self.insert(pos, &s);
                    }
                    _ => unreachable!(),
                },
                crate::container::list::list_op::ListOp::Delete(del) => {
                    self.delete(del.pos as usize..del.pos as usize + del.len as usize);
                }
            },
            _ => unreachable!(),
        }
    }

    #[doc = " Start a transaction"]
    #[doc = ""]
    #[doc = " The transaction may be aborted later, then all the ops during this transaction need to be undone."]
    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        while let Some(op) = self.undo_stack.pop() {
            match op {
                UndoItem::Insert { index, len } => {
                    self.rope
                        .remove(index as usize..index as usize + len as usize);
                }
                UndoItem::Delete {
                    index,
                    byte_offset,
                    len,
                } => {
                    let s = std::str::from_utf8(
                        &self.deleted_bytes
                            [byte_offset as usize..byte_offset as usize + len as usize],
                    )
                    .unwrap();
                    self.rope.insert(index as usize, s);
                }
            }
        }

        self.deleted_bytes.clear();
    }

    fn commit_txn(&mut self) {
        self.deleted_bytes.clear();
        self.undo_stack.clear();
        self.in_txn = false;
    }

    fn get_value(&self) -> LoroValue {
        LoroValue::String(Arc::new(self.rope.to_string()))
    }
}

impl Text {
    pub fn new() -> Self {
        Self {
            rope: JumpRope::new(),
            in_txn: false,
            deleted_bytes: Default::default(),
            undo_stack: Default::default(),
        }
    }

    pub fn insert(&mut self, pos: usize, s: &str) {
        if self.in_txn {
            self.record_insert(pos, s.len());
        }

        self.rope.insert(pos, s);
    }

    pub fn delete(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }

        if range.end > self.len() {
            panic!("delete range out of bound");
        }

        if self.in_txn {
            self.record_del(range.start, range.len());
        }

        self.rope.remove(range);
    }

    fn record_del(&mut self, index: usize, len: usize) {
        let mut start = None;
        for span in self.rope.slice_substrings(index..index + len) {
            if start.is_none() {
                start = Some(self.deleted_bytes.len());
            }
            self.deleted_bytes.extend_from_slice(span.as_bytes());
        }

        self.undo_stack.push(UndoItem::Delete {
            index: index as u32,
            byte_offset: start.unwrap() as u32,
            len: len as u32,
        });
    }

    fn record_insert(&mut self, index: usize, len: usize) {
        self.undo_stack.push(UndoItem::Insert {
            index: index as u32,
            len: len as u32,
        });
    }

    fn len(&self) -> usize {
        self.rope.len_bytes()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn abort_txn() {
        let mut state = Text::new();
        state.insert(0, "haha");
        state.start_txn();
        state.insert(4, "1234");
        state.delete(2..6);
        assert_eq!(state.rope.to_string(), "ha34");
        state.abort_txn();
        assert_eq!(state.rope.to_string(), "haha");
    }
}
