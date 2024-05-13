//! This file contains the methods to maintain the parent-child relationship in Loro.
//! The child-parent relationship is established when the child container is created.
//! It won't be changed. So the depth of a container is fixed.
//!
//!

use loro_common::LoroValue;

use crate::{
    change::Change,
    container::{
        list::list_op::{self, ListOp},
        map::MapSet,
        tree::tree_op::TreeOp,
    },
    op::{ListSlice, RawOp, RawOpContent},
    DocState, OpLog,
};

impl OpLog {
    /// Establish the link between the child container and the parent container.
    /// This relationship will never be broken.
    /// It's used at the entry of creating new ops to ensure all the valid containers
    /// in Loro are properly linked.
    pub(super) fn register_container_and_parent_link(&self, change: &Change) {
        let arena = &self.arena;
        for op in change.ops.iter() {
            match &op.content {
                crate::op::InnerContent::List(l) => match l {
                    list_op::InnerListOp::Insert { slice, .. } => {
                        for v in arena.iter_value_slice(slice.to_range()) {
                            if let LoroValue::Container(c) = v {
                                let idx = arena.register_container(&c);
                                arena.set_parent(idx, Some(op.container));
                            }
                        }
                    }
                    list_op::InnerListOp::Set { value, .. } => {
                        if let LoroValue::Container(c) = value {
                            let idx = arena.register_container(c);
                            arena.set_parent(idx, Some(op.container));
                        }
                    }

                    list_op::InnerListOp::Move { .. } => {}
                    list_op::InnerListOp::InsertText { .. } => {}
                    list_op::InnerListOp::Delete(_) => {}
                    list_op::InnerListOp::StyleStart { .. } => {}
                    list_op::InnerListOp::StyleEnd => {}
                },
                crate::op::InnerContent::Map(m) => {
                    if let Some(LoroValue::Container(c)) = &m.value {
                        let idx = arena.register_container(c);
                        arena.set_parent(idx, Some(op.container));
                    }
                }
                crate::op::InnerContent::Tree(t) => {
                    let id = t.target.associated_meta_container();
                    let idx = arena.register_container(&id);
                    arena.set_parent(idx, Some(op.container));
                }
                crate::op::InnerContent::Future(f) => match &f {
                    #[cfg(feature = "counter")]
                    crate::op::FutureInnerContent::Counter(_) => {}
                    crate::op::FutureInnerContent::Unknown { .. } => {}
                },
            }
        }
    }
}

impl DocState {
    /// This is used in txn to short cut the process of applying an op to the state.
    /// So the op here has not been registered on the oplog yet.
    pub(super) fn set_container_parent_by_raw_op(&self, raw_op: &RawOp) {
        let container = raw_op.container;
        match &raw_op.content {
            RawOpContent::List(op) => {
                if let ListOp::Insert {
                    slice: ListSlice::RawData(list),
                    ..
                } = op
                {
                    let list = match list {
                        std::borrow::Cow::Borrowed(list) => list.iter(),
                        std::borrow::Cow::Owned(list) => list.iter(),
                    };
                    for value in list {
                        if let LoroValue::Container(c) = value {
                            let idx = self.arena.register_container(c);
                            self.arena.set_parent(idx, Some(container));
                        }
                    }
                }
                if let ListOp::Set {
                    elem_id: _,
                    value: LoroValue::Container(c),
                } = op
                {
                    let idx = self.arena.register_container(c);
                    self.arena.set_parent(idx, Some(container));
                }
            }
            RawOpContent::Map(MapSet { key: _, value }) => {
                if let Some(LoroValue::Container(c)) = value {
                    let idx = self.arena.register_container(c);
                    self.arena.set_parent(idx, Some(container));
                }
            }
            RawOpContent::Tree(TreeOp { target, .. }) => {
                // create associated metadata container
                // TODO: maybe we could create map container only when setting metadata
                let container_id = target.associated_meta_container();
                let child_idx = self.arena.register_container(&container_id);
                self.arena.set_parent(child_idx, Some(container));
            }
            #[cfg(feature = "counter")]
            RawOpContent::Counter(_) => {}
            RawOpContent::Unknown { .. } => {}
        }
    }
}
