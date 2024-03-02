use std::sync::{Arc, Mutex, Weak};

use fxhash::FxHashMap;
use loro_common::{CompactIdLp, ContainerID, IdLp, LoroResult, LoroValue};

use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    delta::DeltaItem,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    op::{Op, RawOp},
    txn::Transaction,
    DocState,
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct MovableListState {
    idx: ContainerIdx,
    list: Vec<ListItem>,
    elements: FxHashMap<CompactIdLp, Element>,
}

#[derive(Debug, Clone)]
struct ListItem {
    pointed_by: Option<CompactIdLp>,
    id: IdLp,
}

#[derive(Debug, Clone)]
struct Element {
    value: LoroValue,
    value_id: IdLp,
    pos: IdLp,
}

impl MovableListState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            list: Vec::new(),
            elements: FxHashMap::default(),
        }
    }

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    fn try_update_elem_pos(&mut self, elem: IdLp, new_pos: IdLp) {
        let id = elem.try_into().unwrap();
        if let Some(element) = self.elements.get_mut(&id) {
            if element.pos > new_pos {
                return;
            }

            let _old_pos = element.pos;
            // TODO: update list item pointed by
            element.pos = new_pos;
        } else {
            self.elements.insert(
                id,
                Element {
                    value: LoroValue::Null,
                    value_id: IdLp::NONE_ID,
                    pos: new_pos,
                },
            );
        }
    }

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    fn try_update_elem_value(&mut self, elem: IdLp, value: LoroValue, value_id: IdLp) {
        let id = elem.try_into().unwrap();
        if let Some(element) = self.elements.get_mut(&id) {
            if element.value_id > value_id {
                return;
            }

            element.value = value;
            element.value_id = value_id;
        } else {
            self.elements.insert(
                id,
                Element {
                    value,
                    value_id,
                    pos: IdLp::NONE_ID,
                },
            );
        }
    }

    // TODO: this method should be removed for perf
    fn update(&mut self) {
        let pointed_by: FxHashMap<IdLp, CompactIdLp> = self
            .elements
            .iter()
            .map(|(id, elem)| (elem.pos, *id))
            .collect();
        for item in self.list.iter_mut() {
            item.pointed_by = pointed_by.get(&item.id).copied();
        }
    }
}

impl ContainerState for MovableListState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        todo!()
    }

    fn is_state_empty(&self) -> bool {
        self.list.is_empty() && self.elements.is_empty()
    }

    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let InternalDiff::MovableList(diff) = diff else {
            unreachable!()
        };
        {
            // apply list item changes

            let mut index = 0;
            for delta_item in diff.list.into_iter() {
                match delta_item {
                    DeltaItem::Retain {
                        retain,
                        attributes: _,
                    } => {
                        index += retain;
                    }
                    DeltaItem::Insert {
                        insert,
                        attributes: _,
                    } => {
                        let len = insert.len();
                        self.list.splice(
                            index..index,
                            insert.into_iter().map(|x| ListItem {
                                id: x,
                                pointed_by: None,
                            }),
                        );
                        index += len;
                    }
                    DeltaItem::Delete {
                        delete,
                        attributes: _,
                    } => {
                        self.list.drain(index..index + delete);
                    }
                }
            }
        }

        {
            // apply element changes
            for delta_item in diff.elements.into_iter() {
                match delta_item {
                    crate::delta::ElementDelta::PosChange { id, new_pos } => {
                        self.try_update_elem_pos(id, new_pos);
                    }
                    crate::delta::ElementDelta::ValueChange {
                        id,
                        new_value,
                        value_id,
                    } => self.try_update_elem_value(id, new_value, value_id),
                }
            }
        }

        todo!("Calculate diff")
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) {
        self.apply_diff_and_convert(diff, arena, txn, state);
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()> {
        todo!()
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        todo!()
    }

    fn get_value(&mut self) -> LoroValue {
        let list = self
            .list
            .iter()
            .filter_map(|item| item.pointed_by.map(|eid| self.elements[&eid].value.clone()))
            .collect();
        LoroValue::List(Arc::new(list))
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        todo!()
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        todo!()
    }

    #[doc = r" Encode the ops and the blob that can be used to restore the state to the current state."]
    #[doc = r""]
    #[doc = r" State will use the provided encoder to encode the ops and export a blob."]
    #[doc = r" The ops should be encoded into the snapshot as well as the blob."]
    #[doc = r" The users then can use the ops and the blob to restore the state to the current state."]
    fn encode_snapshot(&self, encoder: StateSnapshotEncoder) -> Vec<u8> {
        todo!()
    }

    #[doc = r" Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        todo!()
    }
}
