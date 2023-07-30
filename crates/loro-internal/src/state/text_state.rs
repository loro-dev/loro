use std::{ops::Range, sync::Arc};

use jumprope::JumpRope;

use crate::{
    arena::SharedArena,
    container::text::text_content::{ListSlice, SliceRanges},
    delta::{Delta, DeltaItem},
    event::Diff,
    op::{RawOp, RawOpContent},
    LoroValue,
};

use super::ContainerState;

#[derive(Debug, Default)]
pub struct TextState {
    pub(crate) rope: JumpRope,
    in_txn: bool,
    deleted_bytes: Vec<u8>,
    undo_stack: Vec<UndoItem>,
}

impl Clone for TextState {
    fn clone(&self) -> Self {
        Self {
            rope: self.rope.clone(),
            in_txn: false,
            deleted_bytes: Default::default(),
            undo_stack: Default::default(),
        }
    }
}

#[derive(Debug)]
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

impl ContainerState for TextState {
    fn apply_diff(&mut self, diff: &mut Diff, arena: &SharedArena) {
        match diff {
            Diff::SeqRaw(delta) => {
                if let Some(new_diff) = self.apply_seq_raw(delta, arena) {
                    *diff = new_diff;
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
                            self.insert_utf8(index, value);
                            index += value.len();
                        }
                        DeltaItem::Delete { len, .. } => {
                            self.delete_utf8(index..index + len);
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn apply_op(&mut self, op: RawOp, _arena: &SharedArena) {
        match op.content {
            RawOpContent::List(list) => match list {
                crate::container::list::list_op::ListOp::Insert { slice, pos } => match slice {
                    ListSlice::RawStr(s) => {
                        self.insert_utf8(pos, &s);
                    }
                    _ => unreachable!(),
                },
                crate::container::list::list_op::ListOp::Delete(del) => {
                    self.delete_utf8(del.pos as usize..del.pos as usize + del.len as usize);
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

    #[doc = " Convert a state to a diff that when apply this diff on a empty state,"]
    #[doc = " the state will be the same as this state."]
    fn to_diff(&self) -> Diff {
        Diff::Text(Delta::new().insert(self.rope.to_string()))
    }
}

impl TextState {
    pub fn new() -> Self {
        Self {
            rope: JumpRope::new(),
            in_txn: false,
            deleted_bytes: Default::default(),
            undo_stack: Default::default(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        let mut state = Self::new();
        state.insert_utf8(0, s);
        state
    }

    pub fn insert_utf8(&mut self, pos: usize, s: &str) {
        if self.in_txn {
            self.record_insert(pos, s.len());
        }

        self.rope.insert(pos, s);
    }

    pub fn delete_utf8(&mut self, range: Range<usize>) {
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

    pub fn len_wchars(&self) -> usize {
        self.rope.len_wchars()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.rope.slice_substrings(0..self.len())
    }

    pub(crate) fn utf16_to_utf8(&self, pos: usize) -> usize {
        self.rope.wchars_to_chars(pos)
    }

    pub fn slice(&self, range: Range<usize>) -> impl Iterator<Item = &str> {
        self.rope.slice_substrings(range)
    }

    #[cfg(not(feature = "wasm"))]
    fn apply_seq_raw(
        &mut self,
        delta: &mut Delta<SliceRanges>,
        arena: &SharedArena,
    ) -> Option<Diff> {
        let mut index = 0;
        for span in delta.iter() {
            match span {
                DeltaItem::Retain { len, meta: _ } => {
                    index += len;
                }
                DeltaItem::Insert { value, .. } => {
                    for value in value.0.iter() {
                        let s = arena.slice_bytes(value.0.start as usize..value.0.end as usize);
                        self.insert_utf8(index, std::str::from_utf8(&s).unwrap());
                        index += s.len();
                    }
                }
                DeltaItem::Delete { len, .. } => {
                    self.delete_utf8(index..index + len);
                }
            }
        }

        None
    }

    #[cfg(feature = "wasm")]
    fn apply_seq_raw(
        &mut self,
        delta: &mut Delta<SliceRanges>,
        arena: &SharedArena,
    ) -> Option<Diff> {
        let mut new_delta = Delta::new();
        let mut index = 0;
        let mut utf16_index = 0;
        for span in delta.iter() {
            match span {
                DeltaItem::Retain { len, meta: _ } => {
                    index += len;
                    let next_utf16_index = self.utf8_to_utf16(index);
                    new_delta = new_delta.retain(next_utf16_index - utf16_index);
                    utf16_index = next_utf16_index;
                }
                DeltaItem::Insert { value, .. } => {
                    new_delta = new_delta.insert(value.clone());
                    let start_utf16_len = self.len_wchars();
                    for value in value.0.iter() {
                        let s = arena.slice_bytes(value.0.start as usize..value.0.end as usize);
                        self.insert_utf8(index, std::str::from_utf8(&s).unwrap());
                        index += s.len();
                    }
                    utf16_index += self.len_wchars() - start_utf16_len;
                }
                DeltaItem::Delete { len, .. } => {
                    let start_utf16_len = self.len_wchars();
                    self.delete_utf8(index..index + len);
                    new_delta = new_delta.delete(start_utf16_len - self.len_wchars());
                }
            }
        }

        Some(Diff::SeqRawUtf16(new_delta))
    }

    fn utf8_to_utf16(&self, index: usize) -> usize {
        self.rope.chars_to_wchars(index)
    }
}

impl std::fmt::Display for TextState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.rope.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn abort_txn() {
        let mut state = TextState::new();
        state.insert_utf8(0, "haha");
        state.start_txn();
        state.insert_utf8(4, "1234");
        state.delete_utf8(2..6);
        assert_eq!(state.rope.to_string(), "ha34");
        state.abort_txn();
        assert_eq!(state.rope.to_string(), "haha");
    }
}
