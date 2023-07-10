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
    delta::{Delta, DeltaItem},
    event::{Diff, Utf16Meta},
    hierarchy::Hierarchy,
    id::{Counter, PeerID},
    log_store::ImportContext,
    op::{InnerContent, Op, RawOpContent, RichOp},
    transaction::Transaction,
    value::LoroValue,
    LoroError, Transact, VersionVector,
};

use super::{
    rope::Rope,
    string_pool::{Alive, PoolString, StringPool},
    text_content::{ListSlice, SliceRange},
    tracker::Tracker,
    utf16::count_utf16_chars,
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

    pub(crate) fn insert(
        &mut self,
        txn: &mut Transaction,
        pos: usize,
        text: &str,
    ) -> Result<(), LoroError> {
        if pos > self.state.len() {
            return Err(LoroError::OutOfBound {
                pos,
                len: self.state.len(),
            });
        }
        let slice = self.raw_str.alloc(text);
        let op_slice = SliceRange::from_pool_string(&slice);
        self.state.insert(pos, slice);
        self._record_insert_op(txn, op_slice, pos, text);
        Ok(())
    }

    pub(crate) fn insert_utf16(
        &mut self,
        txn: &mut Transaction,
        utf16_pos: usize,
        text: &str,
    ) -> Result<(), LoroError> {
        let slice = self.raw_str.alloc(text);
        let op_slice = SliceRange::from_pool_string(&slice);
        let pos = self.state.insert_utf16(utf16_pos, slice)?;
        self._record_insert_op(txn, op_slice, pos, text);
        Ok(())
    }

    pub fn diff(&mut self) {}

    fn _record_insert_op(
        &mut self,
        txn: &mut Transaction,
        op_slice: SliceRange,
        pos: usize,
        text: &str,
    ) {
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
            store.append_local_ops(&[op]);

            if hierarchy.should_notify(&self.id) {
                let utf16_pos = self.state.utf8_to_utf16(pos);
                let delta = Delta::new()
                    .retain_with_meta(pos, Utf16Meta::new(utf16_pos))
                    .insert_with_meta(
                        text.to_owned(),
                        Utf16Meta::new(count_utf16_chars(text.as_bytes())),
                    );
                txn.append_event_diff(&self.id, Diff::Text(delta), true);
            }
        });
    }

    pub(crate) fn delete(&mut self, txn: &mut Transaction, pos: usize, len: usize) {
        if len == 0 {
            return;
        }

        self._record_delete_op(txn, pos, len, None, None);
        self.state.delete_range(Some(pos), Some(pos + len));
    }

    pub(crate) fn delete_utf16(
        &mut self,
        txn: &mut Transaction,
        utf16_pos: usize,
        utf16_len: usize,
    ) -> Result<(), LoroError> {
        if utf16_len == 0 {
            return Ok(());
        }

        let (pos, len) = self.state.delete_utf16(utf16_pos, utf16_len)?;
        self._record_delete_op(txn, pos, len, Some(utf16_pos), Some(utf16_len));
        Ok(())
    }

    fn _record_delete_op(
        &mut self,
        txn: &mut Transaction,
        pos: usize,
        len: usize,
        utf16_pos: Option<usize>,
        utf16_len: Option<usize>,
    ) {
        txn.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let id = store.next_id();
            let op = Op::new(
                id,
                InnerContent::List(InnerListOp::new_del(pos, len)),
                self.idx,
            );
            store.append_local_ops(&[op]);

            if hierarchy.should_notify(&self.id) {
                let utf16_pos = utf16_pos.unwrap_or_else(|| self.state.utf8_to_utf16(pos));
                let utf16_len =
                    utf16_len.unwrap_or_else(|| self.state.utf8_to_utf16(pos + len) - utf16_pos);
                let delta = Delta::new()
                    .retain_with_meta(pos, Utf16Meta::new(utf16_pos))
                    .delete_with_meta(len, Utf16Meta::new(utf16_len));
                txn.append_event_diff(&self.id, Diff::Text(delta), true);
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
            "Text Container {:?}, Raw String size={}, Tree=>",
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

    fn to_export(&mut self, content: InnerContent, gc: bool) -> SmallVec<[RawOpContent; 1]> {
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
                        let v = RawOpContent::List(ListOp::Insert {
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
                                        ans.push(RawOpContent::List(ListOp::Insert {
                                            slice: ListSlice::RawStr(std::borrow::Cow::Owned(
                                                s[start..start + span].to_string(),
                                            )),
                                            pos: pos_start,
                                        }));
                                    }
                                    Alive::False(span) => {
                                        let v = RawOpContent::List(ListOp::Insert {
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
                            ans.push(RawOpContent::List(ListOp::Insert {
                                slice: ListSlice::RawStr(std::borrow::Cow::Owned(s)),
                                pos,
                            }))
                        }
                    }
                }
                InnerListOp::Delete(del) => ans.push(RawOpContent::List(ListOp::Delete(del))),
            },
            InnerContent::Map(_) => unreachable!(),
        }

        assert!(!ans.is_empty());
        ans
    }

    fn to_import(&mut self, content: RawOpContent) -> InnerContent {
        match content {
            RawOpContent::List(list) => match list {
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
                        let s_len = Utf16Meta::new(count_utf16_chars(s.as_bytes()));
                        let delta = Delta::new()
                            .retain_with_meta(
                                *pos,
                                Utf16Meta::new(self.state.utf8_to_utf16_with_unknown(*pos)),
                            )
                            .insert_with_meta(s, s_len);
                        ctx.push_diff(&self.id, Diff::Text(delta));
                    }
                    self.state.insert(
                        *pos,
                        PoolString::from_slice_range(&self.raw_str, slice.clone()),
                    );
                }
                InnerListOp::Delete(span) => {
                    if should_notify {
                        let utf16_pos =
                            self.state.utf8_to_utf16_with_unknown(span.start() as usize);
                        let utf16_end = self.state.utf8_to_utf16_with_unknown(span.end() as usize);
                        let delta = Delta::new()
                            .retain_with_meta(span.start() as usize, Utf16Meta::new(utf16_pos))
                            .delete_with_meta(
                                span.atom_len(),
                                Utf16Meta::new(utf16_end - utf16_pos),
                            );
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
    fn tracker_init(&mut self, vv: &VersionVector) {
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

    fn tracker_checkout(&mut self, vv: &VersionVector) {
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
        let delta = self
            .tracker
            .as_mut()
            .unwrap()
            .diff(&import_context.old_vv, &import_context.new_vv);

        let should_notify = hierarchy.should_notify(&self.id);
        let mut diff = smallvec![];
        let mut index = 0;
        for span in delta.iter() {
            match span {
                DeltaItem::Retain { len, .. } => {
                    index += len;
                }
                DeltaItem::Insert { value: values, .. } => {
                    for value in values.0.iter() {
                        // HACK: after lazifying the event, we can avoid this weird hack
                        if should_notify {
                            let s = if value.is_unknown() {
                                unreachable!()
                                // " ".repeat(value.atom_len())
                            } else {
                                self.raw_str.slice(&value.0).to_owned()
                            };
                            let s_len = Utf16Meta::new(count_utf16_chars(s.as_bytes()));
                            let delta = Delta::new()
                                .retain_with_meta(
                                    index,
                                    Utf16Meta::new(self.state.utf8_to_utf16_with_unknown(index)),
                                )
                                .insert_with_meta(s, s_len);
                            diff.push(Diff::Text(delta));
                        }

                        self.state.insert(
                            index,
                            PoolString::from_slice_range(&self.raw_str, value.clone()),
                        );
                        index += value.atom_len();
                    }
                }
                DeltaItem::Delete { len, .. } => {
                    if should_notify {
                        let utf16_pos = self.state.utf8_to_utf16_with_unknown(index);
                        let utf16_end = self.state.utf8_to_utf16_with_unknown(index + len);
                        let delta = Delta::new()
                            .retain_with_meta(index, Utf16Meta::new(utf16_pos))
                            .delete_with_meta(*len, Utf16Meta::new(utf16_end - utf16_pos));
                        diff.push(Diff::Text(delta));
                    }

                    self.state.delete_range(Some(index), Some(index + len));
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
                let s_len = Utf16Meta::new(count_utf16_chars(s.as_bytes()));
                let delta = Delta::new().insert_with_meta(s, s_len);
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
    client_id: PeerID,
    container_idx: ContainerIdx,
}

impl Text {
    pub fn from_instance(instance: Weak<Mutex<ContainerInstance>>, client_id: PeerID) -> Self {
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
        self.with_transaction(txn, |txn, inner| {
            let len = inner.text_len();
            if len < pos {
                return Err(LoroError::OutOfBound { pos, len });
            }
            inner.insert(txn, pos, text)
        })
    }

    pub fn insert_utf16<T: Transact, S: AsRef<str>>(
        &mut self,
        txn: &T,
        pos: usize,
        text: S,
    ) -> Result<(), crate::LoroError> {
        self.with_transaction(txn, |txn, x| x.insert_utf16(txn, pos, text.as_ref()))
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
            if pos + len > current_length {
                return Err(LoroError::OutOfBound {
                    pos: pos + len,
                    len: current_length,
                });
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
        self.with_transaction(txn, |txn, text| text.delete_utf16(txn, pos, len))
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

    fn client_id(&self) -> crate::id::PeerID {
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
