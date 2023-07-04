use std::sync::{Mutex, Weak};

use rle::{
    rle_tree::{tree_trait::CumulateTreeTrait, HeapMode},
    HasLength, RleTree,
};
use smallvec::{smallvec, SmallVec};

use crate::{
    container::{
        list::list_op::ListOp,
        pool,
        pool_mapping::{PoolMapping, StateContent},
        registry::{ContainerIdx, ContainerInstance, ContainerRegistry, ContainerWrapper},
        text::{
            text_content::{ListSlice, SliceRange},
            tracker::{Effect, Tracker},
        },
        ContainerID, ContainerTrait, ContainerType,
    },
    delta::Delta,
    event::{Diff, Index},
    hierarchy::Hierarchy,
    id::{Counter, PeerID},
    log_store::ImportContext,
    op::{InnerContent, Op, RemoteContent, RichOp},
    prelim::Prelim,
    transaction::Transaction,
    value::LoroValue,
    version::PatchedVersionVector,
    LoroError, Transact, VersionVector,
};

use super::list_op::InnerListOp;

pub(crate) type ListState = RleTree<SliceRange, CumulateTreeTrait<SliceRange, 8, HeapMode>>;

#[derive(Debug)]
pub struct ListContainer {
    id: ContainerID,
    idx: ContainerIdx,
    pub(crate) state: ListState,
    pub(crate) raw_data: pool::Pool,
    tracker: Option<Tracker>,
    pool_mapping: Option<PoolMapping<LoroValue>>,
}

impl ListContainer {
    pub(crate) fn new(id: ContainerID, idx: ContainerIdx) -> Self {
        Self {
            id,
            idx,
            raw_data: pool::Pool::default(),
            tracker: None,
            state: Default::default(),
            pool_mapping: None,
        }
    }

    fn insert<P: Prelim>(
        &mut self,
        txn: &mut Transaction,
        pos: usize,
        value: P,
    ) -> Result<Option<ContainerIdx>, LoroError> {
        let (value, maybe_container) = value.convert_value()?;
        if let Some(prelim) = maybe_container {
            let type_ = value.into_container().unwrap();
            let (id, idx) = txn.register_container(&self.id, type_);
            let value = LoroValue::Container(id.into());
            self.insert_value(txn, pos, value);
            prelim.integrate(txn, idx)?;
            Ok(Some(idx))
        } else {
            let value = value.into_value().unwrap();
            self.insert_value(txn, pos, value);
            Ok(None)
        }
    }

    fn insert_value(&mut self, txn: &mut Transaction, pos: usize, value: LoroValue) {
        txn.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let id = store.next_id();
            let slice = self.raw_data.alloc(value);
            self.state.insert(pos, slice.clone().into());
            let op = Op::new(
                id,
                InnerContent::List(InnerListOp::Insert {
                    slice: slice.clone().into(),
                    pos,
                }),
                self.idx,
            );
            // record op id
            store.append_local_ops(&[op]);

            if hierarchy.should_notify(&self.id) {
                let value = self.raw_data.slice(&slice)[0].clone();
                let delta = Delta::new().retain(pos).insert(vec![value]);
                txn.append_event_diff(&self.id, Diff::List(delta), true);
            }
        });
    }

    pub(crate) fn insert_batch(
        &mut self,
        txn: &mut Transaction,
        pos: usize,
        values: Vec<LoroValue>,
    ) {
        if values.is_empty() {
            return;
        }
        txn.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let slice = self.raw_data.alloc_arr(values);
            // cache event
            if hierarchy.should_notify(&self.id) {
                let values = self.raw_data.slice(&slice).to_vec();
                let delta = Delta::new().retain(pos).insert(values);
                txn.append_event_diff(&self.id, Diff::List(delta), true);
            }
            self.state.insert(pos, slice.clone().into());
            let id = store.next_id();
            let op = Op::new(
                id,
                InnerContent::List(InnerListOp::Insert {
                    slice: slice.into(),
                    pos,
                }),
                self.idx,
            );
            store.append_local_ops(&[op]);
        });
    }

    fn delete(&mut self, txn: &mut Transaction, pos: usize, len: usize) {
        if len == 0 {
            return;
        }

        txn.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let id = store.next_id();
            let op = Op::new(
                id,
                InnerContent::List(InnerListOp::new_del(pos, len)),
                self.idx,
            );
            store.append_local_ops(&[op]);

            if let Some(deleted_containers) = self.update_hierarchy_on_delete(hierarchy, pos, len) {
                deleted_containers.into_iter().for_each(|id| {
                    txn.delete_container(&id);
                })
            }

            self.state.delete_range(Some(pos), Some(pos + len));
            if hierarchy.should_notify(&self.id) {
                let delta = Delta::new().retain(pos).delete(len);
                txn.append_event_diff(&self.id, Diff::List(delta), true);
            }
        });
    }

    pub fn get(&self, pos: usize) -> Option<LoroValue> {
        self.state
            .get(pos)
            .map(|range| self.raw_data.slice(&range.get_sliced_with_len(1).0))
            .and_then(|slice| slice.first().cloned())
    }

    fn update_hierarchy_on_delete(
        &mut self,
        hierarchy: &mut Hierarchy,
        pos: usize,
        len: usize,
    ) -> Option<Vec<ContainerID>> {
        if !hierarchy.has_children(&self.id) {
            return None;
        }
        let mut ans = Vec::new();
        for state in self.state.iter_range(pos, Some(pos + len)) {
            let range = &state.get_sliced().0;

            if SliceRange::from(range.start..range.end).is_unknown() {
                continue;
            }
            for value in self.raw_data.slice(range).iter() {
                if let LoroValue::Container(container_id) = value {
                    debug_log::debug_log!("Deleted {:?}", container_id);
                    hierarchy.remove_child(&self.id, container_id);
                    ans.push(container_id.as_ref().clone());
                }
            }
        }
        Some(ans)
    }

    pub fn values_len(&self) -> usize {
        self.state.len()
    }

    pub fn check(&mut self) {
        if let Some(x) = self.tracker.as_mut() {
            x.check()
        }
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&mut self) {
        println!(
            "Text Container {:?}, Raw String size={}, Tree=>\n",
            self.id,
            self.raw_data.len(),
        );
        self.state.debug_inspect();
    }

    /// TODO: perf, can store the position info to the container children
    pub fn index_of_child(&self, child: &ContainerID) -> Option<Index> {
        let mut idx = 0;
        for values in self.state.iter() {
            let value = values.as_ref();
            for v in self.raw_data.slice(&value.0) {
                if v.as_container().map(|x| &**x == child).unwrap_or(false) {
                    return Some(Index::Seq(idx));
                }

                idx += 1;
            }
        }

        None
    }

    fn update_hierarchy_on_insert(&mut self, hierarchy: &mut Hierarchy, content: &SliceRange) {
        for value in self.raw_data.slice(&content.0).iter() {
            if let LoroValue::Container(container_id) = value {
                hierarchy.add_child(&self.id, container_id);
            }
        }
    }

    pub fn to_json(&self, reg: &ContainerRegistry) -> LoroValue {
        self.get_value().resolve_deep(reg)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        self.state
            .iter()
            .flat_map(|c| self.raw_data.slice(&c.as_ref().0).iter())
    }
}

impl ContainerTrait for ListContainer {
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
        let mut values = Vec::new();
        for range in self.state.iter() {
            let content = range.as_ref();
            for value in self.raw_data.slice(&content.0) {
                values.push(value.clone());
            }
        }
        values.into()
    }

    fn to_export(&mut self, content: InnerContent, _gc: bool) -> SmallVec<[RemoteContent; 1]> {
        match content {
            InnerContent::List(list) => match list {
                InnerListOp::Insert { slice, pos } => {
                    if slice.is_unknown() {
                        let v = RemoteContent::List(ListOp::Insert {
                            slice: ListSlice::Unknown(slice.atom_len()),
                            pos,
                        });
                        smallvec::smallvec![v]
                    } else {
                        let data = self.raw_data.slice(&slice.0);
                        smallvec::smallvec![RemoteContent::List(ListOp::Insert {
                            pos,
                            slice: ListSlice::RawData(data.to_vec()),
                        })]
                    }
                }
                InnerListOp::Delete(del) => {
                    smallvec::smallvec![RemoteContent::List(ListOp::Delete(del))]
                }
            },
            InnerContent::Map(_) => {
                unreachable!()
            }
        }
    }

    fn to_import(&mut self, content: RemoteContent) -> InnerContent {
        match content {
            RemoteContent::List(list) => match list {
                ListOp::Insert { slice, pos } => match slice {
                    ListSlice::RawData(data) => {
                        let slice_range = self.raw_data.alloc_arr(data);
                        let slice: SliceRange = slice_range.into();
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

    fn update_state_directly(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        context: &mut ImportContext,
    ) {
        let should_notify = hierarchy.should_notify(&self.id);
        match &op.get_sliced().content {
            InnerContent::List(op) => match op {
                InnerListOp::Insert { slice, pos } => {
                    if should_notify {
                        let delta = Delta::new();
                        // unknown
                        let delta_vec = if slice.is_unknown() {
                            let mut ans = Vec::with_capacity(slice.atom_len());
                            for _ in 0..slice.content_len() {
                                ans.push(LoroValue::Null);
                            }
                            ans
                        } else {
                            self.raw_data.slice(&slice.0).to_vec()
                        };
                        let delta = delta.retain(*pos).insert(delta_vec);
                        context.push_diff(&self.id, Diff::List(delta));
                    }
                    if !slice.is_unknown() {
                        self.update_hierarchy_on_insert(hierarchy, slice);
                    }
                    self.state.insert(*pos, slice.clone());
                }
                InnerListOp::Delete(span) => {
                    if should_notify {
                        let delta = Delta::new()
                            .retain(span.start() as usize)
                            .delete(span.atom_len());
                        context.push_diff(&self.id, Diff::List(delta));
                    }

                    self.update_hierarchy_on_delete(
                        hierarchy,
                        span.start() as usize,
                        span.atom_len(),
                    );
                    self.state
                        .delete_range(Some(span.start() as usize), Some(span.end() as usize));
                }
            },
            _ => unreachable!(),
        }
    }

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
        let should_notify = hierarchy.should_notify(&self.id);
        let mut diff = smallvec![];
        for effect in self
            .tracker
            .as_mut()
            .unwrap()
            .iter_effects(&import_context.old_vv, &import_context.spans)
        {
            match effect {
                Effect::Del { pos, len } => {
                    if should_notify {
                        let delta = Delta::new().retain(pos).delete(len);
                        diff.push(Diff::List(delta));
                    }

                    if hierarchy.has_children(&self.id) {
                        // update hierarchy
                        for state in self.state.iter_range(pos, Some(pos + len)) {
                            let range = state.get_sliced();
                            if !range.is_unknown() {
                                for value in self.raw_data.slice(&range.0).iter() {
                                    if let LoroValue::Container(container_id) = value {
                                        debug_log::debug_log!("Deleted {:?}", container_id);
                                        hierarchy.remove_child(&self.id, container_id);
                                    }
                                }
                            }
                        }
                    }

                    self.state.delete_range(Some(pos), Some(pos + len));
                }
                Effect::Ins { pos, content } => {
                    if should_notify {
                        let s = if content.is_unknown() {
                            (0..content.atom_len()).map(|_| LoroValue::Null).collect()
                        } else {
                            self.raw_data.slice(&content.0).to_vec()
                        };
                        let delta = Delta::new().retain(pos).insert(s);
                        diff.push(Diff::List(delta));
                    }
                    if !content.is_unknown() {
                        for value in self.raw_data.slice(&content.0).iter() {
                            // update hierarchy
                            if let LoroValue::Container(container_id) = value {
                                hierarchy.add_child(&self.id, container_id);
                            }
                        }
                    }

                    self.state.insert(pos, content);
                }
            }
        }

        if should_notify {
            import_context.push_diff_vec(&self.id, diff);
        }

        self.tracker = None;
    }

    fn to_export_snapshot(
        &mut self,
        content: &InnerContent,
        gc: bool,
    ) -> SmallVec<[InnerContent; 1]> {
        let old_pool = if gc {
            None
        } else {
            Some(self.raw_data.as_slice())
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

    fn initialize_pool_mapping(&mut self) {
        let mut pool_mapping = PoolMapping::default();
        for value in self.state.iter() {
            pool_mapping.push_state_slice(value.get_sliced().0, self.raw_data.as_slice());
        }
        pool_mapping.push_state_slice_finish();
        self.pool_mapping = Some(pool_mapping);
    }

    fn to_import_snapshot(
        &mut self,
        state_content: StateContent,
        hierarchy: &mut Hierarchy,
        ctx: &mut ImportContext,
    ) {
        if let StateContent::List { pool, state_len } = state_content {
            for v in pool.iter() {
                if let LoroValue::Container(child_container_id) = v {
                    hierarchy.add_child(self.id(), child_container_id.as_ref());
                }
            }
            self.raw_data = pool.into();
            self.state.insert(0, (0..state_len).into());
            // notify
            let should_notify = hierarchy.should_notify(&self.id);
            if should_notify {
                let delta_vec = self.raw_data.slice(&(0..state_len)).to_vec();
                let delta = Delta::new().insert(delta_vec);

                ctx.push_diff(&self.id, Diff::List(delta));
            }
        } else {
            unreachable!()
        }
    }

    fn encode_and_release_pool_mapping(&mut self) -> StateContent {
        let pool_mapping = self.pool_mapping.take().unwrap();
        let state_len = pool_mapping.new_state_len;
        StateContent::List {
            pool: pool_mapping.inner(),
            state_len,
        }
    }
}

#[derive(Debug, Clone)]
pub struct List {
    container: Weak<Mutex<ContainerInstance>>,
    client_id: PeerID,
    container_idx: ContainerIdx,
}

impl List {
    pub fn from_instance(instance: Weak<Mutex<ContainerInstance>>, client_id: PeerID) -> Self {
        let container_idx = {
            let list = instance.upgrade().unwrap();
            let list = list.try_lock().unwrap();
            list.idx()
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

    /// Inserts an element at position index within the List
    pub fn insert<T: Transact, P: Prelim>(
        &mut self,
        txn: &T,
        pos: usize,
        value: P,
    ) -> Result<Option<ContainerIdx>, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let len = x.values_len();
            if len < pos{
                return Err(LoroError::TransactionError(
                    format!(
                        "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                        self.container_idx, len, pos
                    )
                    .into(),
                ));
            }
            x.insert(txn, pos, value)
        })
    }

    /// Inserts some elements at position index within the List
    pub fn insert_batch<T: Transact>(
        &mut self,
        txn: &T,
        pos: usize,
        values: Vec<LoroValue>,
    ) -> Result<(), LoroError> {
        self.with_transaction(txn, |txn, x| {
            let len = x.values_len();
            if len < pos{
                return Err(LoroError::TransactionError(
                    format!(
                        "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                        self.container_idx, len, pos
                    )
                    .into(),
                ));
            }
            x.insert_batch(txn, pos, values);
            Ok(())
        })
    }

    /// Appends an element to the back
    pub fn push<T: Transact, P: Prelim>(
        &mut self,
        txn: &T,
        value: P,
    ) -> Result<Option<ContainerIdx>, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let pos = x.values_len();
            x.insert(txn, pos, value)
        })
    }

    // Inserts an element to the front
    pub fn push_front<T: Transact, P: Prelim>(
        &mut self,
        txn: &T,
        value: P,
    ) -> Result<Option<ContainerIdx>, LoroError> {
        self.with_transaction(txn, |txn, x| x.insert(txn, 0, value))
    }

    /// Removes the last element from the List and returns it, or None if it is empty.
    pub fn pop<T: Transact>(&mut self, txn: &T) -> Result<Option<LoroValue>, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let len = x.values_len();
            if len == 0 {
                return Ok(None);
            }
            let value = x.get(len - 1);
            x.delete(txn, len - 1, 1);
            Ok(value)
        })
    }

    /// Removes the specified range (pos..pos+len) from the List
    pub fn delete<T: Transact>(
        &mut self,
        txn: &T,
        pos: usize,
        len: usize,
    ) -> Result<(), LoroError> {
        self.with_transaction(txn, |txn, x| {
            let current_length = x.values_len();
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

    /// return the value of the element at that position or None if out of bounds.
    pub fn get(
        &self,
        // txn: &T,
        pos: usize,
    ) -> Option<LoroValue> {
        // self.with_transaction(txn, |txn, x| Ok(x.get(pos)))
        self.with_container(|x| x.get(pos))
    }

    pub fn len(&self) -> usize {
        // self.with_transaction(txn, |txn, x| Ok(x.values_len()))
        self.with_container(|x| x.values_len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn for_each<F: FnMut((usize, &LoroValue))>(&self, f: F) {
        self.with_container(|list| list.iter().enumerate().for_each(f))
    }

    // TODO
    pub fn map<F: FnMut((usize, &LoroValue)) -> R, R>(&self, f: F) -> Vec<R> {
        self.with_container(|list| list.iter().enumerate().map(f).collect())
    }

    /// Need clone
    pub fn id(&self) -> ContainerID {
        self.with_container(|list| list.id.clone())
    }

    pub fn get_value(&self) -> LoroValue {
        self.with_container(|x| x.get_value())
    }
}

impl ContainerWrapper for List {
    type Container = ListContainer;

    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let w = self.container.upgrade().unwrap();
        let mut container_instance = w.try_lock().unwrap();
        let list = container_instance.as_list_mut().unwrap();
        f(list)
    }

    fn client_id(&self) -> PeerID {
        self.client_id
    }

    fn idx(&self) -> ContainerIdx {
        self.container_idx
    }
}

#[cfg(test)]
mod test {
    use crate::{LoroCore, Transact};

    #[test]
    fn test_list_get() {
        let mut loro = LoroCore::default();
        let mut list = loro.get_list("id");
        {
            let txn = loro.transact();
            list.insert(&txn, 0, 123).unwrap();
            list.insert(&txn, 1, 123).unwrap();
        }
        assert_eq!(list.get(0), Some(123.into()));
        assert_eq!(list.get(1), Some(123.into()));
    }

    #[test]
    #[cfg(feature = "json")]
    fn collection() {
        let mut loro = LoroCore::default();
        let mut list = loro.get_list("list");
        list.insert(&loro, 0, "ab").unwrap();
        assert_eq!(list.get_value().to_json(), "[\"ab\"]");
        list.push(&loro, 12).unwrap();
        assert_eq!(list.get_value().to_json(), "[\"ab\",12]");
        list.push_front(&loro, -3).unwrap();
        assert_eq!(list.get_value().to_json(), "[-3,\"ab\",12]");
        let last = list.pop(&loro).unwrap().unwrap();
        assert_eq!(last.to_json(), "12");
        assert_eq!(list.get_value().to_json(), "[-3,\"ab\"]");
        list.delete(&loro, 1, 1).unwrap();
        assert_eq!(list.get_value().to_json(), "[-3]");
        list.insert_batch(&loro, 1, vec!["cd".into(), 123.into()])
            .unwrap();
        assert_eq!(list.get_value().to_json(), "[-3,\"cd\",123]");
        list.delete(&loro, 0, 3).unwrap();
        assert_eq!(list.get_value().to_json(), "[]");
        assert_eq!(list.pop(&loro).unwrap(), None);
    }

    #[test]
    #[cfg(feature = "json")]
    fn for_each() {
        let mut loro = LoroCore::default();
        let mut list = loro.get_list("list");
        list.insert(&loro, 0, "a").unwrap();
        list.insert(&loro, 1, "b").unwrap();
        list.insert(&loro, 2, "c").unwrap();
        list.for_each(|(idx, v)| {
            let c = match idx {
                0 => "a",
                1 => "b",
                2 => "c",
                _ => unreachable!(),
            };
            assert_eq!(format!("\"{c}\""), v.to_json())
        })
    }

    #[test]
    #[cfg(feature = "json")]
    fn map() {
        let mut loro = LoroCore::default();
        let mut list = loro.get_list("list");
        list.insert(&loro, 0, "a").unwrap();
        list.insert(&loro, 1, "b").unwrap();
        list.insert(&loro, 2, "c").unwrap();
        // list.insert(&loro, 3, PrelimContainer::from("hello".to_string()))
        //     .unwrap();
        assert_eq!(
            list.map(|(_, v)| v.to_json()),
            vec!["\"a\"", "\"b\"", "\"c\""]
        );
    }
}
