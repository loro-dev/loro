use std::{
    ops::{Deref, Range},
    sync::Arc,
};

use fxhash::FxHashMap;
use generic_btree::rle::HasLength;
use loro_common::LoroValue;

use crate::{
    arena::SharedArena,
    container::richtext::{AnchorType, RichtextState as InnerState, StyleOp},
    container::{list::list_op, richtext::richtext_state::RichtextStateChunk},
    delta::DeltaItem,
    event::Diff,
    op::{Op, RawOp},
};

use super::ContainerState;

#[derive(Debug)]
pub struct RichtextState {
    state: InnerState,
    in_txn: bool,
    undo_stack: Vec<UndoItem>,
}

impl Clone for RichtextState {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            in_txn: false,
            undo_stack: Vec::new(),
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
        content: RichtextStateChunk,
    },
}

impl ContainerState for RichtextState {
    fn apply_diff(&mut self, diff: &mut Diff, arena: &SharedArena) {
        let Diff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        let mut index = 0;
        let mut style_starts: FxHashMap<Arc<StyleOp>, usize> = FxHashMap::default();
        for span in richtext.vec.iter() {
            match span {
                crate::delta::DeltaItem::Retain { len, meta } => {
                    index += len;
                }
                crate::delta::DeltaItem::Insert { value, meta } => {
                    match value {
                        RichtextStateChunk::Text { unicode_len, text } => {
                            self.state.insert_elem_at_entity_index(
                                index,
                                RichtextStateChunk::Text {
                                    unicode_len: *unicode_len,
                                    text: text.clone(),
                                },
                            );
                        }
                        RichtextStateChunk::Style { style, anchor_type } => {
                            self.state.insert_elem_at_entity_index(
                                index,
                                RichtextStateChunk::Style {
                                    style: style.clone(),
                                    anchor_type: *anchor_type,
                                },
                            );

                            if *anchor_type == AnchorType::Start {
                                style_starts.insert(style.clone(), index);
                            } else {
                                let start_pos =
                                    style_starts.get(style).expect("Style start not found");
                                // we need to + 1 because we also need to annotate the end anchor
                                self.state
                                    .annotate_style_range(*start_pos..index + 1, style.clone());
                            }
                        }
                    }
                    self.undo_stack.push(UndoItem::Insert {
                        index: index as u32,
                        len: value.rle_len() as u32,
                    });
                    index += value.rle_len();
                }
                crate::delta::DeltaItem::Delete { len, meta } => {
                    let content = self.state.drain_by_entity_index(index, *len);
                    for span in content {
                        self.undo_stack.push(UndoItem::Delete {
                            index: index as u32,
                            content: span,
                        })
                    }
                }
            }
        }
    }

    fn apply_op(&mut self, _: &RawOp, op: &Op, arena: &SharedArena) {
        match &op.content {
            crate::op::InnerContent::List(l) => match l {
                list_op::InnerListOp::Insert { slice, pos } => {
                    self.state.insert_at_entity_index(
                        *pos,
                        arena.slice_by_unicode(slice.0.start as usize..slice.0.end as usize),
                    );
                }
                list_op::InnerListOp::Delete(del) => {
                    self.state
                        .delete_with_entity_index(del.pos as usize, del.len as usize);
                }
                list_op::InnerListOp::StyleStart { start, end, style } => {
                    self.state
                        .mark_with_entity_index(*start as usize..*end as usize, style.clone());
                }
                list_op::InnerListOp::StyleEnd => {}
            },
            _ => unreachable!(),
        }
    }

    fn to_diff(&self) -> Diff {
        let mut delta = crate::delta::Delta::new();
        for span in self.state.iter_chunk() {
            delta.vec.push(DeltaItem::Insert {
                value: span.clone(),
                meta: (),
            })
        }

        Diff::RichtextRaw(delta)
    }

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        self.undo_all();
    }

    fn commit_txn(&mut self) {
        self.in_txn = false;
        self.undo_stack.clear();
    }

    // value is a list
    fn get_value(&self) -> LoroValue {
        LoroValue::String(Arc::new(self.state.to_string()))
    }
}

impl RichtextState {
    pub fn new() -> Self {
        Self {
            state: InnerState::default(),
            in_txn: false,
            undo_stack: Vec::new(),
        }
    }

    fn undo_all(&mut self) {
        while let Some(item) = self.undo_stack.pop() {
            match item {
                UndoItem::Insert { index, len } => {
                    let _ = self
                        .state
                        .drain_by_entity_index(index as usize, len as usize);
                }
                UndoItem::Delete { index, content } => {
                    match content {
                        RichtextStateChunk::Text { .. } => {}
                        RichtextStateChunk::Style { .. } => {
                            unimplemented!("should handle style annotation")
                        }
                    }

                    self.state
                        .insert_elem_at_entity_index(index as usize, content);
                }
            }
        }
    }

    pub fn len_utf16(&self) -> usize {
        self.state.len_utf16()
    }

    pub fn len_entity(&self) -> usize {
        self.state.len_entity()
    }

    pub fn len_unicode(&self) -> usize {
        self.state.len_unicode()
    }

    pub(crate) fn get_entity_index_for_text_insert_pos(&self, pos: usize) -> usize {
        self.state.get_entity_index_for_text_insert_pos(pos)
    }

    pub(crate) fn get_text_entity_ranges_in_unicode_range(
        &self,
        pos: usize,
        len: usize,
    ) -> Vec<Range<usize>> {
        self.state.get_text_entity_ranges_in_unicode_range(pos, len)
    }

    pub fn get_richtext_value(&self) -> LoroValue {
        self.state.get_richtext_value()
    }
}
