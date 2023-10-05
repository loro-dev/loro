use loro_common::LoroValue;

use crate::{
    arena::SharedArena, container::richtext::richtext_state::Elem,
    container::richtext::RichtextState as InnerState, event::Diff, op::RawOp,
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
    Insert { index: u32, len: u32 },
    Delete { index: u32, content: Elem },
}

impl ContainerState for RichtextState {
    fn apply_diff(&mut self, diff: &mut Diff, arena: &SharedArena) {
        let Diff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        let mut index = 0;
        for span in richtext.vec.iter() {
            match span {
                crate::delta::DeltaItem::Retain { len, meta } => {
                    index += len;
                }
                crate::delta::DeltaItem::Insert { value, meta } => {
                    match value.value() {
                        crate::container::richtext::RichtextChunkValue::Text(r) => {
                            self.state.insert(
                                index,
                                arena.slice_by_unicode(r.start as usize..r.end as usize),
                            );
                        }
                        crate::container::richtext::RichtextChunkValue::Symbol(s) => {
                            unimplemented!()
                        }
                        crate::container::richtext::RichtextChunkValue::Unknown(_) => {
                            unreachable!()
                        }
                    }
                    self.undo_stack.push(UndoItem::Insert {
                        index: index as u32,
                        len: value.len() as u32,
                    });
                    index += value.len();
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

    fn apply_op(&mut self, op: RawOp, arena: &SharedArena) {
        match &op.content {
            crate::op::RawOpContent::List(list) => match list {
                crate::container::list::list_op::ListOp::Insert { slice, pos } => {}
                crate::container::list::list_op::ListOp::Delete(_) => todo!(),
                crate::container::list::list_op::ListOp::Style {
                    start,
                    end,
                    key,
                    info,
                } => todo!(),
            },
            _ => unreachable!(),
        }
    }

    fn to_diff(&self) -> Diff {
        todo!()
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
        todo!()
    }
}

impl RichtextState {
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
                        Elem::Text { .. } => {}
                        Elem::Style(_) => unimplemented!("should handle style annotation"),
                    }

                    self.state
                        .insert_elem_at_entity_index(index as usize, content);
                }
            }
        }
    }
}
