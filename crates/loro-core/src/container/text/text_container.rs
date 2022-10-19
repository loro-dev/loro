use fxhash::FxHashMap;
use rle::RleVec;

use smallvec::SmallVec;

use crate::{
    container::{list::list_op::ListOp, Container, ContainerID, ContainerType},
    id::ID,
    log_store::LogStoreWeakRef,
    op::{InsertContent, Op, OpContent, OpProxy},
    span::IdSpan,
    value::LoroValue,
    ClientID,
};

use super::text_content::ListSlice;

#[derive(Clone, Debug)]
struct DagNode {
    id: IdSpan,
    deps: SmallVec<[ID; 2]>,
}

#[derive(Clone, Debug)]
pub struct TextContainer {
    id: ContainerID,
    sub_dag: FxHashMap<ClientID, RleVec<DagNode, ()>>,
    log_store: LogStoreWeakRef,
    raw: String,
}

impl TextContainer {
    pub fn insert(&mut self, pos: usize, text: &str) -> Option<ID> {
        let upgraded = self.log_store.upgrade()?;
        let id = if let Ok(mut store) = upgraded.write() {
            let id = store.next_id();
            let slice = ListSlice::new(store.raw_str.alloc(text));
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

    pub fn delete(&mut self, _pos: usize, _len: usize) {}
}

impl Container for TextContainer {
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn type_(&self) -> ContainerType {
        ContainerType::Text
    }

    fn apply(&mut self, _op: &OpProxy) {
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
