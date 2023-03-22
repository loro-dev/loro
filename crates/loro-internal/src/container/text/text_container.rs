use std::sync::{Mutex, Weak};

use append_only_bytes::AppendOnlyBytes;
use rle::HasLength;
use smallvec::{smallvec, SmallVec};
use tracing::instrument;

use crate::{
    container::{
        list::list_op::{InnerListOp, ListOp},
        pool_mapping::{PoolMapping, StateContent},
        registry::{ContainerIdx, ContainerInstance, ContainerWrapper},
        ContainerID, ContainerTrait, ContainerType,
    },
    delta::Delta,
    event::Diff,
    hierarchy::Hierarchy,
    id::{ClientID, Counter},
    log_store::ImportContext,
    op::{InnerContent, Op, RemoteContent, RichOp},
    transaction::Transaction,
    value::LoroValue,
    version::PatchedVersionVector,
    LoroError, Transact,
};

use super::{
    rope::Rope,
    string_pool::{Alive, PoolString, StringPool},
    text_content::{ListSlice, SliceRange},
    tracker::{Effect, Tracker},
};

#[derive(Debug)]
pub struct TextContainer {
    id: ContainerID,
    idx: ContainerIdx,
    state: Rope,
    raw_str: StringPool,
    tracker: Option<Tracker>,
    pool_mapping: Option<PoolMapping<u8>>,
}

impl TextContainer {
    pub(crate) fn new(id: ContainerID, idx: ContainerIdx) -> Self {
        Self {
            id,
            idx,
            raw_str: StringPool::default(),
            tracker: None,
            state: Default::default(),
            pool_mapping: None,
        }
    }

    pub(crate) fn insert(&mut self, txn: &mut Transaction, pos: usize, text: &str) {
        let slice = self.raw_str.alloc(text);
        let op_slice = SliceRange::from_pool_string(&slice);
        self.state.insert(pos, slice);
        txn.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let id = store.next_id();
            let op = Op::new(
                id,
                InnerContent::List(InnerListOp::Insert {
                    slice: op_slice,
                    pos,
                }),
                self.idx,
            );
            txn.push(self.idx, id);
            store.append_local_ops(&[op]);
            txn.update_version(store.frontiers().into());

            if hierarchy.should_notify(&self.id) {
                let delta = Delta::new().retain(pos).insert(text.to_owned());
                txn.append_event_diff(self.idx, Diff::Text(delta), true);
            }
        });
    }

    pub(crate) fn delete(&mut self, txn: &mut Transaction, pos: usize, len: usize) {
        self.state.delete_range(Some(pos), Some(pos + len));
        txn.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let id = store.next_id();
            let op = Op::new(
                id,
                InnerContent::List(InnerListOp::new_del(pos, len)),
                self.idx,
            );
            txn.push(self.idx, id);
            store.append_local_ops(&[op]);
            txn.update_version(store.frontiers().into());

            if hierarchy.should_notify(&self.id) {
                let delta = Delta::new().retain(pos).delete(len);
                txn.append_event_diff(self.idx, Diff::Text(delta), true);
            }
        });
    }

    pub fn text_len(&self) -> usize {
        self.state.len()
    }

    pub fn check(&mut self) {
        if let Some(x) = self.tracker.as_mut() {
            x.check()
        }
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&mut self) {
        let pool = &self.raw_str;
        println!(
            "Text Container {:?}, Raw String size={}, Tree=>\n",
            self.id,
            pool.len(),
        );
        self.state.debug_inspect();
    }

    pub fn to_json(&self) -> LoroValue {
        self.get_value()
    }
}

impl ContainerTrait for TextContainer {
    #[inline(always)]
    fn id(&self) -> &ContainerID {
        &self.id
    }

    #[inline(always)]
    fn idx(&self) -> ContainerIdx {
        self.idx
    }

    #[inline(always)]
    fn type_(&self) -> ContainerType {
        ContainerType::Text
    }

    // TODO: maybe we need to let this return Cow
    fn get_value(&self) -> LoroValue {
        let mut ans_str = String::new();
        for v in self.state.iter() {
            let content = v.as_ref();
            if content.is_unknown() {
                panic!("Unknown range when getting value");
            }

            ans_str.push_str(content.as_str_unchecked());
        }

        LoroValue::String(ans_str.into_boxed_str())
    }

    fn to_export(&mut self, content: InnerContent, gc: bool) -> SmallVec<[RemoteContent; 1]> {
        if gc && self.raw_str.should_update_aliveness(self.text_len()) {
            self.raw_str
                .update_aliveness(self.state.iter().filter_map(|x| {
                    x.as_ref()
                        .slice
                        .as_ref()
                        .map(|x| x.start() as u32..x.end() as u32)
                }))
        }

        let mut ans = SmallVec::new();
        match content {
            InnerContent::List(list) => match list {
                InnerListOp::Insert { slice, pos } => {
                    let r = slice;
                    if r.is_unknown() {
                        let v = RemoteContent::List(ListOp::Insert {
                            slice: ListSlice::Unknown(r.atom_len()),
                            pos,
                        });
                        ans.push(v);
                    } else {
                        let s = self.raw_str.get_string(&r.0);
                        if gc {
                            let mut start = 0;
                            let mut pos_start = pos;
                            for span in self.raw_str.get_aliveness(&r.0) {
                                match span {
                                    Alive::True(span) => {
                                        ans.push(RemoteContent::List(ListOp::Insert {
                                            slice: ListSlice::RawStr(s[start..start + span].into()),
                                            pos: pos_start,
                                        }));
                                    }
                                    Alive::False(span) => {
                                        let v = RemoteContent::List(ListOp::Insert {
                                            slice: ListSlice::Unknown(span),
                                            pos: pos_start,
                                        });
                                        ans.push(v);
                                    }
                                }

                                start += span.atom_len();
                                pos_start += span.atom_len();
                            }
                            assert_eq!(start, r.atom_len());
                        } else {
                            ans.push(RemoteContent::List(ListOp::Insert {
                                slice: ListSlice::RawStr(s),
                                pos,
                            }))
                        }
                    }
                }
                InnerListOp::Delete(del) => ans.push(RemoteContent::List(ListOp::Delete(del))),
            },
            InnerContent::Map(_) => unreachable!(),
        }

        assert!(!ans.is_empty());
        ans
    }

    fn to_import(&mut self, content: RemoteContent) -> InnerContent {
        match content {
            RemoteContent::List(list) => match list {
                ListOp::Insert { slice, pos } => match slice {
                    ListSlice::RawStr(s) => {
                        let range = self.raw_str.alloc(&s);
                        let slice: SliceRange = SliceRange::from_pool_string(&range);
                        InnerContent::List(InnerListOp::Insert { slice, pos })
                    }
                    ListSlice::Unknown(u) => InnerContent::List(InnerListOp::Insert {
                        slice: SliceRange::new_unknown(u as u32),
                        pos,
                    }),
                    _ => unreachable!(),
                },
                ListOp::Delete(del) => InnerContent::List(InnerListOp::Delete(del)),
            },
            _ => unreachable!(),
        }
    }

    #[instrument(skip_all)]
    fn update_state_directly(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        ctx: &mut ImportContext,
    ) {
        let should_notify = hierarchy.should_notify(&self.id);
        match &op.get_sliced().content {
            InnerContent::List(op) => match op {
                InnerListOp::Insert { slice, pos } => {
                    if should_notify {
                        // HACK: after lazifying the event, we can avoid this weird hack
                        let s = if slice.is_unknown() {
                            " ".repeat(slice.atom_len())
                        } else {
                            self.raw_str.slice(&slice.0).to_owned()
                        };
                        let delta = Delta::new().retain(*pos).insert(s);
                        ctx.push_diff(&self.id, Diff::Text(delta));
                    }
                    self.state.insert(
                        *pos,
                        PoolString::from_slice_range(&self.raw_str, slice.clone()),
                    );
                }
                InnerListOp::Delete(span) => {
                    if should_notify {
                        let delta = Delta::new()
                            .retain(span.start() as usize)
                            .delete(span.atom_len());
                        ctx.push_diff(&self.id, Diff::Text(delta));
                    }

                    self.state
                        .delete_range(Some(span.start() as usize), Some(span.end() as usize))
                }
            },
            _ => unreachable!(),
        }
    }

    #[instrument(skip_all)]
    fn tracker_init(&mut self, vv: &PatchedVersionVector) {
        match &mut self.tracker {
            Some(tracker) => {
                if (!vv.is_empty() || tracker.start_vv().is_empty())
                    && tracker.all_vv() >= vv
                    && vv >= tracker.start_vv()
                {
                } else {
                    self.tracker = Some(Tracker::new(vv.clone(), Counter::MAX / 2));
                }
            }
            None => {
                self.tracker = Some(Tracker::new(vv.clone(), Counter::MAX / 2));
            }
        }
    }

    fn tracker_checkout(&mut self, vv: &PatchedVersionVector) {
        self.tracker.as_mut().unwrap().checkout(vv)
    }

    fn track_apply(&mut self, _: &mut Hierarchy, rich_op: &RichOp, _: &mut ImportContext) {
        self.tracker.as_mut().unwrap().track_apply(rich_op);
    }

    fn apply_tracked_effects_from(
        &mut self,
        hierarchy: &mut Hierarchy,
        import_context: &mut ImportContext,
    ) {
        let should_notify = hierarchy.should_notify(&self.id);
        let mut diff = smallvec![];
        for effect in self.tracker.as_mut().unwrap().iter_effects(
            import_context.patched_old_vv.as_ref().unwrap(),
            &import_context.spans,
        ) {
            match effect {
                Effect::Del { pos, len } => {
                    if should_notify {
                        let delta = Delta::new().retain(pos).delete(len);
                        diff.push(Diff::Text(delta));
                    }

                    self.state.delete_range(Some(pos), Some(pos + len));
                }
                Effect::Ins { pos, content } => {
                    // HACK: after lazifying the event, we can avoid this weird hack
                    if should_notify {
                        let s = if content.is_unknown() {
                            " ".repeat(content.atom_len())
                        } else {
                            self.raw_str.slice(&content.0).to_owned()
                        };
                        let delta = Delta::new().retain(pos).insert(s);
                        diff.push(Diff::Text(delta));
                    }

                    self.state
                        .insert(pos, PoolString::from_slice_range(&self.raw_str, content));
                }
            }
        }

        if should_notify {
            import_context.push_diff_vec(&self.id, diff);
        }

        self.tracker = None;
    }

    fn initialize_pool_mapping(&mut self) {
        let mut pool_mapping = PoolMapping::default();
        for value in self.state.iter() {
            let range = value.get_sliced().slice.unwrap();
            let range = range.start() as u32..range.end() as u32;
            let old = self.raw_str.as_bytes();
            pool_mapping.push_state_slice(range, old);
        }
        pool_mapping.push_state_slice_finish();
        self.pool_mapping = Some(pool_mapping);
    }

    fn encode_and_release_pool_mapping(&mut self) -> StateContent {
        let pool_mapping = self.pool_mapping.take().unwrap();
        let state_len = pool_mapping.new_state_len;
        StateContent::Text {
            pool: pool_mapping.inner(),
            state_len,
        }
    }

    fn to_export_snapshot(
        &mut self,
        content: &InnerContent,
        gc: bool,
    ) -> SmallVec<[InnerContent; 1]> {
        let old_pool = if gc {
            None
        } else {
            Some(self.raw_str.as_bytes())
        };
        match content {
            InnerContent::List(op) => match op {
                InnerListOp::Insert { slice, pos } => {
                    let new_slice = self
                        .pool_mapping
                        .as_mut()
                        .unwrap()
                        .convert_ops_slice(slice.0.clone(), old_pool);
                    let mut offset = 0;
                    new_slice
                        .into_iter()
                        .map(|slice| {
                            let ans = InnerContent::List(InnerListOp::Insert {
                                slice,
                                pos: *pos + offset,
                            });
                            offset += ans.atom_len();
                            ans
                        })
                        .collect()
                }
                InnerListOp::Delete(span) => {
                    SmallVec::from([InnerContent::List(InnerListOp::Delete(*span))])
                }
            },
            _ => unreachable!(),
        }
    }

    fn to_import_snapshot(
        &mut self,
        state_content: StateContent,
        hierarchy: &mut Hierarchy,
        ctx: &mut ImportContext,
    ) {
        if let StateContent::Text { pool, state_len } = state_content {
            let mut append_only_bytes = AppendOnlyBytes::with_capacity(pool.len());
            append_only_bytes.push_slice(&pool);
            let pool_string = append_only_bytes.slice(0..state_len as usize).into();
            self.raw_str = StringPool::from_data(append_only_bytes);
            self.state.insert(0, pool_string);
            // notify
            let should_notify = hierarchy.should_notify(&self.id);
            if should_notify {
                let s = self.raw_str.slice(&(0..state_len)).to_owned();
                let delta = Delta::new().retain(0).insert(s);
                ctx.push_diff(&self.id, Diff::Text(delta));
            }
        } else {
            unreachable!()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Text {
    container: Weak<Mutex<ContainerInstance>>,
    client_id: ClientID,
    container_idx: ContainerIdx,
}

impl Text {
    pub fn from_instance(instance: Weak<Mutex<ContainerInstance>>, client_id: ClientID) -> Self {
        let container_idx = {
            let x = instance.upgrade().unwrap();
            let x = x.try_lock().unwrap();
            x.idx()
        };
        Self {
            container: instance,
            client_id,
            container_idx,
        }
    }

    #[inline(always)]
    pub fn idx(&self) -> ContainerIdx {
        self.container_idx
    }

    #[inline(always)]
    pub fn id(&self) -> ContainerID {
        self.with_container(|x| x.id.clone())
    }

    pub fn insert<T: Transact, S: AsRef<str>>(
        &mut self,
        txn: &T,
        pos: usize,
        text: S,
    ) -> Result<(), crate::LoroError> {
        let text = text.as_ref();
        if text.is_empty() {
            return Ok(());
        }
        self.with_transaction(txn, |txn, x| {
            let len = x.text_len();
            if len < pos{
                return Err(LoroError::TransactionError(
                    format!(
                        "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                        self.container_idx, len, pos
                    )
                    .into(),
                ));
            }
            x.insert(txn, pos, text);
            Ok(())
        })
    }

    pub fn insert_utf16<T: Transact, S: AsRef<str>>(
        &mut self,
        txn: &T,
        pos: usize,
        text: S,
    ) -> Result<(), crate::LoroError> {
        self.with_transaction(txn, |txn, x| {
            let pos = x.state.utf16_to_utf8(pos);
            let len = x.text_len();
            if len < pos{
                return Err(LoroError::TransactionError(
                    format!(
                        "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                        self.container_idx, len, pos
                    )
                    .into(),
                ));
            }
            x.insert(txn, pos, text.as_ref());
            Ok(())
        })
    }

    pub fn delete<T: Transact>(
        &mut self,
        txn: &T,
        pos: usize,
        len: usize,
    ) -> Result<(), crate::LoroError> {
        if len == 0 {
            return Ok(());
        }
        self.with_transaction(txn, |txn, x| {
            let current_length = x.text_len();
            if pos > current_length {
                return Err(LoroError::TransactionError(
                    format!(
                        "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                        self.container_idx, len, pos
                    )
                    .into(),
                ));
            }
            if pos + len > current_length {
                return Err(LoroError::TransactionError(
                    format!("`ContainerIdx-{:?}` can not apply delete op: the current len is {} but the delete range is {:?}", self.container_idx, current_length, pos..pos+len).into(),
                ));
            }
            x.delete(txn, pos, len);
            Ok(())
        })
    }

    pub fn delete_utf16<T: Transact>(
        &mut self,
        txn: &T,
        pos: usize,
        len: usize,
    ) -> Result<(), crate::LoroError> {
        self.with_transaction(txn, |txn, text| {
            let end = pos + len;
            let pos = text.state.utf16_to_utf8(pos);
            let len = text.state.utf16_to_utf8(end) - pos;
            let current_length = text.text_len();
            if pos > current_length {
                return Err(LoroError::TransactionError(
                    format!(
                        "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                        self.container_idx, len, pos
                    )
                    .into(),
                ));
            }
            if pos + len > current_length {
                return Err(LoroError::TransactionError(
                    format!("`ContainerIdx-{:?}` can not apply delete op: the current len is {} but the delete range is {:?}", self.container_idx, current_length, pos..pos+len).into(),
                ));
            }
            text.delete(txn, pos, len);
            Ok(())
        })
    }

    pub fn get_value(&self) -> LoroValue {
        self.with_container(|x| x.get_value())
    }

    pub fn len(&self) -> usize {
        self.with_container(|x| x.text_len())
    }

    pub fn len_utf16(&self) -> usize {
        self.with_container(|x| x.state.utf8_to_utf16(x.text_len()))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ContainerWrapper for Text {
    type Container = TextContainer;

    fn client_id(&self) -> crate::id::ClientID {
        self.client_id
    }

    #[inline(always)]
    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let w = self.container.upgrade().unwrap();
        let mut container_instance = w.try_lock().unwrap();
        let x = container_instance.as_text_mut().unwrap();
        f(x)
    }

    fn idx(&self) -> ContainerIdx {
        self.container_idx
    }
}
