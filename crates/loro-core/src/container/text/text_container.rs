use rle::RleTree;
use smallvec::SmallVec;

use crate::{
    container::{list::list_op::ListOp, Container, ContainerID, ContainerType},
    id::ID,
    log_store::LogStoreRef,
    op::{InsertContent, Op, OpContent, OpProxy},
    span::IdSpan,
    value::LoroValue,
};

use super::{
    string_pool::StringPool,
    text_content::{ListSlice, ListSliceTreeTrait},
    tracker::Tracker,
};

#[derive(Clone, Debug)]
struct DagNode {
    id: IdSpan,
    deps: SmallVec<[ID; 2]>,
}

#[derive(Debug)]
pub struct TextContainer {
    id: ContainerID,
    log_store: LogStoreRef,
    state: RleTree<ListSlice, ListSliceTreeTrait>,
    raw_str: StringPool,
    tracker: Tracker,
}

impl TextContainer {
    pub fn insert(&mut self, pos: usize, text: &str) -> Option<ID> {
        let id = if let Ok(mut store) = self.log_store.write() {
            let id = store.next_id();
            let slice = ListSlice::from_range(self.raw_str.alloc(text));
            self.state.insert(pos, slice.clone());
            let op = Op::new(
                id,
                OpContent::Normal {
                    content: InsertContent::List(ListOp::Insert { slice, pos }),
                },
                self.id.clone(),
            );
            store.append_local_ops(vec![op]);
            id
        } else {
            unimplemented!()
        };

        Some(id)
    }

    pub fn delete(&mut self, pos: usize, len: usize) -> Option<ID> {
        let id = if let Ok(mut store) = self.log_store.write() {
            let id = store.next_id();
            let op = Op::new(
                id,
                OpContent::Normal {
                    content: InsertContent::List(ListOp::Delete { len, pos }),
                },
                self.id.clone(),
            );

            store.append_local_ops(vec![op]);
            self.state.delete_range(Some(pos), Some(pos + len));
            id
        } else {
            unimplemented!()
        };

        Some(id)
    }
}

impl Container for TextContainer {
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn type_(&self) -> ContainerType {
        ContainerType::Text
    }

    fn apply(&mut self, op: &OpProxy) {
        let _content = op.content_sliced();
        todo!()
    }

    fn checkout_version(&mut self, _vv: &crate::VersionVector) {
        todo!()
    }

    fn get_value(&mut self) -> &LoroValue {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
