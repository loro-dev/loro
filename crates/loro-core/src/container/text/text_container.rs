use colored::Colorize;
use rle::{RleTree, Sliceable};
use smallvec::{smallvec, SmallVec};

use crate::{
    container::{list::list_op::ListOp, Container, ContainerID, ContainerType},
    dag::DagUtils,
    debug_log,
    id::ID,
    log_store::LogStoreWeakRef,
    op::{InsertContent, Op, OpContent},
    smstring::SmString,
    span::{HasIdSpan, IdSpan},
    value::LoroValue,
    LogStore, VersionVector,
};

use super::{
    string_pool::StringPool,
    text_content::{ListSlice, ListSliceTreeTrait},
    tracker::{Effect, Tracker},
};

#[derive(Clone, Debug)]
struct DagNode {
    id: IdSpan,
    deps: SmallVec<[ID; 2]>,
}

#[derive(Debug)]
pub struct TextContainer {
    id: ContainerID,
    log_store: LogStoreWeakRef,
    state: RleTree<ListSlice, ListSliceTreeTrait>,
    raw_str: StringPool,
    tracker: Tracker,
    state_cache: LoroValue,

    head: SmallVec<[ID; 2]>,
    vv: VersionVector,
}

impl TextContainer {
    pub fn new(id: ContainerID, log_store: LogStoreWeakRef) -> Self {
        Self {
            id,
            log_store,
            raw_str: StringPool::default(),
            tracker: Tracker::new(Default::default()),
            state_cache: LoroValue::Null,
            state: Default::default(),
            // TODO: should be eq to log_store frontier?
            head: Default::default(),
            vv: Default::default(),
        }
    }

    pub fn insert(&mut self, pos: usize, text: &str) -> Option<ID> {
        if text.is_empty() {
            return None;
        }

        let id = if let Ok(mut store) = self.log_store.upgrade().unwrap().write() {
            let id = store.next_id();
            // let slice = ListSlice::from_range(self.raw_str.alloc(text));
            let slice = ListSlice::from_raw(SmString::from(text));
            self.state.insert(pos, slice.clone());
            let op = Op::new(
                id,
                OpContent::Normal {
                    content: InsertContent::List(ListOp::Insert { slice, pos }),
                },
                self.id.clone(),
            );
            let last_id = op.id_last();
            store.append_local_ops(vec![op]);
            self.head = smallvec![last_id];
            self.vv.set_last(last_id);
            id
        } else {
            unimplemented!()
        };

        Some(id)
    }

    pub fn delete(&mut self, pos: usize, len: usize) -> Option<ID> {
        if len == 0 {
            return None;
        }

        let id = if let Ok(mut store) = self.log_store.upgrade().unwrap().write() {
            let id = store.next_id();
            let op = Op::new(
                id,
                OpContent::Normal {
                    content: InsertContent::List(ListOp::Delete { len, pos }),
                },
                self.id.clone(),
            );

            let last_id = op.id_last();
            store.append_local_ops(vec![op]);
            self.state.delete_range(Some(pos), Some(pos + len));
            self.head = smallvec![last_id];
            self.vv.set_last(last_id);
            id
        } else {
            unimplemented!()
        };

        Some(id)
    }

    pub fn text_len(&self) -> usize {
        self.state.len()
    }

    pub fn check(&mut self) {
        self.tracker.check();
    }
}

impl Container for TextContainer {
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn type_(&self) -> ContainerType {
        ContainerType::Text
    }

    // TODO: move main logic to tracker module
    // TODO: we don't need op proxy, only ids are enough
    fn apply(&mut self, id_span: IdSpan, store: &LogStore) {
        debug_log!("APPLY ENTRY client={}", store.this_client_id);
        let new_op_id = id_span.id_last();
        // TODO: may reduce following two into one op
        let common_ancestors = store.find_common_ancestor(&[new_op_id], &self.head);
        let path_to_head = store.find_path(&common_ancestors, &self.head);
        let mut common_ancestors_vv = self.vv.clone();
        common_ancestors_vv.retreat(&path_to_head.right);
        let mut latest_head: SmallVec<[ID; 2]> = self.head.clone();
        latest_head.push(new_op_id);
        if common_ancestors.is_empty()
            || !common_ancestors.iter().all(|x| self.tracker.contains(*x))
        {
            debug_log!("NewTracker");
            self.tracker = Tracker::new(common_ancestors_vv);
        } else {
            debug_log!("OldTracker");
            self.tracker.checkout(&common_ancestors_vv);
        }

        // stage 1
        let path = store.find_path(&common_ancestors, &latest_head);
        // TODO: need a better mechanism to track the head (KEEP IT IN TRACKER?)
        for iter in store.iter_partial(&common_ancestors, path.right) {
            self.tracker.retreat(&iter.retreat);
            self.tracker.forward(&iter.forward);
            // TODO: avoid this clone
            let change = iter
                .data
                .slice(iter.slice.start as usize, iter.slice.end as usize);
            debug_log!(
                "Stage1 retreat:{} forward:{}\n{}",
                format!("{:?}", &iter.retreat).red(),
                format!("{:?}", &iter.forward).red(),
                format!("{:#?}", &change).blue(),
            );
            for op in change.ops.iter() {
                if op.container == self.id {
                    // TODO: convert op to local
                    self.tracker.apply(op.id, &op.content)
                }
            }
        }

        // stage 2
        // TODO: reduce computations
        let path = store.find_path(&self.head, &latest_head);
        self.tracker.checkout(&self.vv);
        debug_log!(
            "Iterate path: {} from {} => {}",
            format!("{:?}", path.left).red(),
            format!("{:?}", self.head).red(),
            format!("{:?}", latest_head).red(),
        );
        for effect in self.tracker.iter_effects(path.right) {
            debug_log!("EFFECT: {:?}", &effect);
            match effect {
                Effect::Del { pos, len } => self.state.delete_range(Some(pos), Some(pos + len)),
                Effect::Ins { pos, content } => {
                    self.state.insert(pos, content);
                }
            }
        }

        self.head.push(new_op_id);
        self.vv.set_last(new_op_id);
        debug_log!("--------------------------------");
    }

    fn checkout_version(&mut self, _vv: &crate::VersionVector) {
        todo!()
    }

    // TODO: maybe we need to let this return Cow
    fn get_value(&mut self) -> &LoroValue {
        let mut ans_str = SmString::new();
        for v in self.state.iter() {
            let content = v.as_ref();
            match content {
                ListSlice::Slice(range) => ans_str.push_str(&self.raw_str.get_str(range)),
                ListSlice::RawStr(raw) => ans_str.push_str(raw),
                _ => unreachable!(),
            }
        }

        self.state_cache = LoroValue::String(ans_str);
        &self.state_cache
    }

    fn to_export(&self, op: &mut Op) {
        if let Some((slice, _pos)) = op
            .content
            .as_normal_mut()
            .and_then(|c| c.as_list_mut())
            .and_then(|x| x.as_insert_mut())
        {
            let change = if let ListSlice::Slice(ranges) = slice {
                Some(self.raw_str.get_str(ranges))
            } else {
                None
            };

            if let Some(change) = change {
                *slice = ListSlice::RawStr(change);
            }
        }
    }
}
