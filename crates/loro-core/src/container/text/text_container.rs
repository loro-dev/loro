use std::sync::{Arc, Mutex};

use rle::{
    rle_tree::{tree_trait::CumulateTreeTrait, HeapMode},
    HasLength, RleTree, RleVec, Sliceable,
};
use smallvec::{smallvec, SmallVec};

use crate::{
    container::{
        list::list_op::ListOp,
        registry::{ContainerInstance, ContainerWrapper},
        Container, ContainerID, ContainerType,
    },
    context::Context,
    dag::DagUtils,
    debug_log,
    id::{Counter, ID},
    op::{Content, Op, OpContent, RemoteOp},
    span::{HasCounterSpan, HasIdSpan, IdSpan},
    value::LoroValue,
    LogStore,
};

use super::{
    string_pool::{Alive, StringPool},
    text_content::{ListSlice, SliceRange},
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
    state: RleTree<SliceRange, CumulateTreeTrait<SliceRange, 8, HeapMode>>,
    raw_str: StringPool,
    tracker: Tracker,
    head: SmallVec<[ID; 2]>,
}

impl TextContainer {
    pub(crate) fn new(id: ContainerID) -> Self {
        Self {
            id,
            raw_str: StringPool::default(),
            tracker: Tracker::new(Default::default(), 0),
            state: Default::default(),
            // TODO: should be eq to log_store frontier?
            head: Default::default(),
        }
    }

    pub fn insert<C: Context>(&mut self, ctx: &C, pos: usize, text: &str) -> Option<ID> {
        if text.is_empty() {
            return None;
        }

        if self.state.len() < pos {
            panic!("insert index out of range");
        }

        let store = ctx.log_store();
        let mut store = store.write().unwrap();
        let id = store.next_id();
        let slice = self.raw_str.alloc(text);
        self.state.insert(pos, slice.clone().into());
        let op = Op::new(
            id,
            OpContent::Normal {
                content: Content::List(ListOp::Insert {
                    slice: slice.into(),
                    pos,
                }),
            },
            store.get_or_create_container_idx(&self.id),
        );
        let last_id = ID::new(
            store.this_client_id,
            op.counter + op.atom_len() as Counter - 1,
        );
        store.append_local_ops(&[op]);
        self.head = smallvec![last_id];

        Some(id)
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, pos: usize, len: usize) -> Option<ID> {
        if len == 0 {
            return None;
        }

        if self.state.len() < pos + len {
            panic!("deletion out of range");
        }

        let store = ctx.log_store();
        let mut store = store.write().unwrap();
        let id = store.next_id();
        let op = Op::new(
            id,
            OpContent::Normal {
                content: Content::List(ListOp::new_del(pos, len)),
            },
            store.get_or_create_container_idx(&self.id),
        );

        let last_id = ID::new(store.this_client_id, op.ctr_last());
        store.append_local_ops(&[op]);
        self.state.delete_range(Some(pos), Some(pos + len));
        self.head = smallvec![last_id];
        Some(id)
    }

    pub fn text_len(&self) -> usize {
        self.state.len()
    }

    pub fn check(&mut self) {
        self.tracker.check();
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&mut self) {
        println!(
            "Text Container {:?}, Raw String size={}, Tree=>\n",
            self.id,
            self.raw_str.len(),
        );
        self.state.debug_inspect();
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
    fn apply(&mut self, id_span: IdSpan, store: &LogStore) {
        debug_log!("APPLY ENTRY client={}", store.this_client_id);
        let self_idx = store.get_container_idx(&self.id).unwrap();
        let new_op_id = id_span.id_last();
        // TODO: may reduce following two into one op
        let common_ancestors = store.find_common_ancestor(&[new_op_id], &self.head);
        let vv = store.get_vv();
        if common_ancestors == self.head {
            let latest_head = smallvec![new_op_id];
            let path = store.find_path(&self.head, &latest_head);
            if path.right.len() == 1 {
                // linear updates, we can apply them directly
                let start = vv.get(&new_op_id.client_id).copied().unwrap_or(0);
                for op in store.iter_ops_at_id_span(
                    IdSpan::new(new_op_id.client_id, start, new_op_id.counter + 1),
                    self.id.clone(),
                ) {
                    let op = op.get_sliced();
                    match &op.content {
                        OpContent::Normal {
                            content: Content::List(op),
                        } => match op {
                            ListOp::Insert { slice, pos } => {
                                let v = match slice {
                                    ListSlice::Slice(slice) => slice.clone(),
                                    ListSlice::Unknown(u) => ListSlice::unknown_range(*u),
                                    _ => unreachable!(),
                                };

                                self.state.insert(*pos, v)
                            }
                            ListOp::Delete(span) => self.state.delete_range(
                                Some(span.start() as usize),
                                Some(span.end() as usize),
                            ),
                        },
                        _ => unreachable!(),
                    }
                }

                self.head = latest_head;
                return;
            } else {
                let path: Vec<_> = store.iter_partial(&self.head, path.right).collect();
                if path
                    .iter()
                    .all(|x| x.forward.is_empty() && x.retreat.is_empty())
                {
                    // if we don't need to retreat or forward, we can update the state directly
                    for iter in path {
                        let change = iter
                            .data
                            .slice(iter.slice.start as usize, iter.slice.end as usize);
                        for op in change.ops.iter() {
                            if op.container == self_idx {
                                match &op.content {
                                    OpContent::Normal {
                                        content: Content::List(op),
                                    } => match op {
                                        ListOp::Insert { slice, pos } => {
                                            let v = match slice {
                                                ListSlice::Slice(slice) => slice.clone(),
                                                ListSlice::Unknown(u) => {
                                                    ListSlice::unknown_range(*u)
                                                }
                                                _ => unreachable!(),
                                            };

                                            self.state.insert(*pos, v)
                                        }
                                        ListOp::Delete(span) => self.state.delete_range(
                                            Some(span.start() as usize),
                                            Some(span.end() as usize),
                                        ),
                                    },
                                    _ => unreachable!(),
                                }
                            }
                        }
                    }

                    self.head = latest_head;
                    return;
                }
            }
        }

        let path_to_head = store.find_path(&common_ancestors, &self.head);
        let mut common_ancestors_vv = vv.clone();
        common_ancestors_vv.retreat(&path_to_head.right);
        let mut latest_head: SmallVec<[ID; 2]> = self.head.clone();
        latest_head.retain(|x| !common_ancestors_vv.includes_id(*x));
        latest_head.push(new_op_id);
        // println!("{}", store.mermaid());
        debug_log!(
            "START FROM HEADS={:?} new_op_id={} self.head={:?}",
            &common_ancestors,
            new_op_id,
            &self.head
        );

        let tracker_head = if (common_ancestors.is_empty() && !self.tracker.start_vv().is_empty())
            || !common_ancestors.iter().all(|x| self.tracker.contains(*x))
        {
            debug_log!("NewTracker");
            self.tracker = Tracker::new(common_ancestors_vv, Counter::MAX / 2);
            common_ancestors
        } else {
            debug_log!("OldTracker");
            self.tracker.checkout_to_latest();
            self.tracker.all_vv().get_head()
        };

        // stage 1
        let path = store.find_path(&tracker_head, &latest_head);
        debug_log!("path={:?}", &path.right);
        for iter in store.iter_partial(&tracker_head, path.right) {
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
            self.tracker.retreat(&iter.retreat);
            self.tracker.forward(&iter.forward);
            for op in change.ops.iter() {
                if op.container == self_idx {
                    // TODO: convert op to local
                    self.tracker.apply(
                        ID {
                            client_id: change.id.client_id,
                            counter: op.counter,
                        },
                        &op.content,
                    )
                }
            }
        }

        // stage 2
        // TODO: reduce computations
        let path = store.find_path(&self.head, &latest_head);
        debug_log!("BEFORE CHECKOUT");
        // dbg!(&self.tracker);
        self.tracker.checkout(vv);
        debug_log!("AFTER CHECKOUT");
        // dbg!(&self.tracker);
        debug_log!(
            "[Stage 2]: Iterate path: {} from {} => {}",
            format!("{:?}", path.right).red(),
            format!("{:?}", self.head).red(),
            format!("{:?}", latest_head).red(),
        );
        debug_log!(
            "BEFORE EFFECT STATE={}",
            self.get_value().as_string().unwrap()
        );
        for effect in self.tracker.iter_effects(path.right) {
            debug_log!("EFFECT: {:?}", &effect);
            match effect {
                Effect::Del { pos, len } => self.state.delete_range(Some(pos), Some(pos + len)),
                Effect::Ins { pos, content } => {
                    let v = match content {
                        ListSlice::Slice(slice) => slice.clone(),
                        ListSlice::Unknown(u) => ListSlice::unknown_range(u),
                        _ => unreachable!(),
                    };

                    self.state.insert(pos, v)
                }
            }
            debug_log!("AFTER EFFECT");
        }
        debug_log!(
            "AFTER EFFECT STATE={}",
            self.get_value().as_string().unwrap()
        );

        self.head = latest_head;
        debug_log!("--------------------------------");
    }

    fn checkout_version(&mut self, _vv: &crate::VersionVector) {
        todo!()
    }

    // TODO: maybe we need to let this return Cow
    fn get_value(&self) -> LoroValue {
        let mut ans_str = String::new();
        for v in self.state.iter() {
            let content = v.as_ref();
            if SliceRange::is_unknown(content) {
                panic!("Unknown range when getting value");
            }

            ans_str.push_str(&self.raw_str.get_str(&content.0));
        }

        LoroValue::String(ans_str.into_boxed_str())
    }

    fn to_export(&mut self, op: &mut RemoteOp, gc: bool) {
        if gc && self.raw_str.should_update_aliveness(self.text_len()) {
            self.raw_str
                .update_aliveness(self.state.iter().map(|x| x.as_ref().0.clone()))
        }

        let mut contents: RleVec<[OpContent; 1]> = RleVec::new();
        for content in op.contents.iter_mut() {
            if let Some((slice, pos)) = content
                .as_normal_mut()
                .and_then(|c| c.as_list_mut())
                .and_then(|x| x.as_insert_mut())
            {
                match slice {
                    ListSlice::Slice(r) => {
                        if r.is_unknown() {
                            panic!("Unknown range in state");
                        }

                        let s = self.raw_str.get_str(&r.0);
                        if gc {
                            let mut start = 0;
                            let mut pos_start = *pos;
                            for span in self.raw_str.get_aliveness(&r.0) {
                                match span {
                                    Alive::True(span) => {
                                        contents.push(OpContent::Normal {
                                            content: Content::List(ListOp::Insert {
                                                slice: ListSlice::RawStr(
                                                    s[start..start + span].into(),
                                                ),
                                                pos: pos_start,
                                            }),
                                        });
                                    }
                                    Alive::False(span) => {
                                        let v = OpContent::Normal {
                                            content: Content::List(ListOp::Insert {
                                                slice: ListSlice::Unknown(span),
                                                pos: pos_start,
                                            }),
                                        };
                                        contents.push(v);
                                    }
                                }

                                start += span.atom_len();
                                pos_start += span.atom_len();
                            }
                            assert_eq!(start, r.atom_len());
                        } else {
                            contents.push(OpContent::Normal {
                                content: Content::List(ListOp::Insert {
                                    slice: ListSlice::RawStr(s),
                                    pos: *pos,
                                }),
                            });
                        }
                    }
                    ListSlice::Unknown(u) => {
                        let data = OpContent::Normal {
                            content: Content::List(ListOp::Insert {
                                slice: ListSlice::Unknown(*u),
                                pos: *pos,
                            }),
                        };

                        contents.push(data);
                    }
                    _ => unreachable!(),
                }
            } else {
                contents.push(content.clone());
            }
        }

        op.contents = contents;
    }

    fn to_import(&mut self, op: &mut RemoteOp) {
        for content in op.contents.iter_mut() {
            if let Some((slice, _pos)) = content
                .as_normal_mut()
                .and_then(|c| c.as_list_mut())
                .and_then(|x| x.as_insert_mut())
            {
                if let Some(slice_range) = match slice {
                    ListSlice::RawStr(s) => {
                        let range = self.raw_str.alloc(s);
                        Some(range)
                    }
                    ListSlice::Unknown(_) => None,
                    ListSlice::Slice(_) => unreachable!(),
                    ListSlice::RawData(_) => unreachable!(),
                } {
                    *slice = slice_range.into();
                }
            }
        }
    }
}

pub struct Text {
    instance: Arc<Mutex<ContainerInstance>>,
}

impl Clone for Text {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
        }
    }
}

impl Text {
    pub fn insert<C: Context>(&mut self, ctx: &C, pos: usize, text: &str) -> Option<ID> {
        self.with_container(|x| x.insert(ctx, pos, text))
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, pos: usize, len: usize) -> Option<ID> {
        self.with_container(|text| text.delete(ctx, pos, len))
    }

    // TODO: can be len?
    pub fn text_len(&self) -> usize {
        self.with_container(|text| text.text_len())
    }
}

impl ContainerWrapper for Text {
    type Container = TextContainer;

    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let mut container_instance = self.instance.lock().unwrap();
        let map = container_instance.as_text_mut().unwrap();
        f(map)
    }
}

impl From<Arc<Mutex<ContainerInstance>>> for Text {
    fn from(text: Arc<Mutex<ContainerInstance>>) -> Self {
        Text { instance: text }
    }
}
