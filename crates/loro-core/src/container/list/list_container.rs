// TODO: refactor, extract common code with text
use std::sync::{Arc, Mutex};

use rle::{
    rle_tree::{tree_trait::CumulateTreeTrait, HeapMode},
    HasLength, RleTree,
};
use smallvec::SmallVec;

use crate::{
    container::{
        list::list_op::ListOp,
        pool,
        registry::{ContainerInstance, ContainerRegistry, ContainerWrapper},
        text::{
            text_content::{ListSlice, SliceRange},
            tracker::{Effect, Tracker},
        },
        Container, ContainerID, ContainerType,
    },
    context::Context,
    delta::Delta,
    event::{Diff, Index, RawEvent},
    hierarchy::Hierarchy,
    id::{ClientID, Counter, ID},
    log_store::ImportContext,
    op::{InnerContent, Op, RemoteContent, RichOp},
    prelim::Prelim,
    value::LoroValue,
    version::IdSpanVector,
    LogStore, LoroError,
};

use super::list_op::InnerListOp;

#[derive(Debug)]
pub struct ListContainer {
    id: ContainerID,
    state: RleTree<SliceRange, CumulateTreeTrait<SliceRange, 8, HeapMode>>,
    raw_data: pool::Pool,
    tracker: Tracker,
}

impl ListContainer {
    pub(crate) fn new(id: ContainerID) -> Self {
        Self {
            id,
            raw_data: pool::Pool::default(),
            tracker: Tracker::new(Default::default(), 0),
            state: Default::default(),
        }
    }

    pub fn insert_batch<C: Context>(&mut self, ctx: &C, pos: usize, values: Vec<LoroValue>) {
        if values.is_empty() {
            return;
        }
        assert!(!values.iter().any(|x|x.as_unresolved().is_some()), "Cannot have containers in insert_batch method. If you want to create sub container, please use insert_obj or insert method");
        let store = ctx.log_store();
        let mut store = store.try_write().unwrap();

        let id = store.next_id();

        let slice = self.raw_data.alloc_arr(values);
        self.state.insert(pos, slice.clone().into());
        let op = Op::new(
            id,
            InnerContent::List(InnerListOp::Insert {
                slice: slice.into(),
                pos,
            }),
            store.get_or_create_container_idx(&self.id),
        );
        store.append_local_ops(&[op]);
    }

    pub fn insert<C: Context, P: Prelim>(
        &mut self,
        ctx: &C,
        pos: usize,
        value: P,
    ) -> Option<ContainerID> {
        let (value, maybe_container) = value.convert_value();
        if let Some(prelim) = maybe_container {
            let container_id = self.insert_obj(ctx, pos, value.into_container().unwrap());
            let m = ctx.log_store();
            let store = m.read().unwrap();
            let container = Arc::clone(store.get_container(&container_id).unwrap());
            drop(store);
            prelim.integrate(ctx, &container);
            Some(container_id)
        } else {
            let value = value.into_value().unwrap();
            self.insert_value(ctx, pos, value);
            None
        }
    }

    fn insert_value<C: Context>(&mut self, ctx: &C, pos: usize, value: LoroValue) -> Option<ID> {
        let store = ctx.log_store();
        let mut store = store.write().unwrap();
        let id = store.next_id();
        let slice = self.raw_data.alloc(value);
        self.state.insert(pos, slice.clone().into());
        let op = Op::new(
            id,
            InnerContent::List(InnerListOp::Insert {
                slice: slice.clone().into(),
                pos,
            }),
            store.get_or_create_container_idx(&self.id),
        );
        let (old_version, new_version) = store.append_local_ops(&[op]);
        let new_version = new_version.into();
        if store.hierarchy.should_notify(&self.id) {
            let value = self.raw_data.slice(&slice)[0].clone();
            let mut delta = Delta::new();
            delta.retain(pos);
            delta.insert(vec![value]);
            self.notify_local(
                &mut store,
                vec![Diff::List(delta)],
                old_version,
                new_version,
            )
        }

        Some(id)
    }

    fn insert_obj<C: Context>(&mut self, ctx: &C, pos: usize, obj: ContainerType) -> ContainerID {
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
        let (container_id, _) = store.create_container(obj);
        // Update hierarchy info
        store.hierarchy.add_child(&self.id, &container_id);

        // TODO: we can avoid this lock
        drop(store);
        self.insert(
            ctx,
            pos,
            LoroValue::Unresolved(Box::new(container_id.clone())),
        );

        container_id
    }

    pub fn get(&self, pos: usize) -> Option<LoroValue> {
        self.state
            .get(pos)
            .map(|range| self.raw_data.slice(&range.as_ref().0))
            .and_then(|slice| slice.first().cloned())
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
            InnerContent::List(InnerListOp::new_del(pos, len)),
            store.get_or_create_container_idx(&self.id),
        );

        let (old_version, new_version) = store.append_local_ops(&[op]);
        let new_version = new_version.into();
        // Update hierarchy info
        self.update_hierarchy_on_delete(&mut store.hierarchy, pos, len);

        self.state.delete_range(Some(pos), Some(pos + len));

        if store.hierarchy.should_notify(&self.id) {
            let mut delta = Delta::new();
            delta.retain(pos);
            delta.delete(len);
            self.notify_local(
                &mut store,
                vec![Diff::List(delta)],
                old_version,
                new_version,
            )
        }

        Some(id)
    }

    fn notify_local(
        &mut self,
        store: &mut LogStore,
        diff: Vec<Diff>,
        old_version: SmallVec<[ID; 2]>,
        new_version: SmallVec<[ID; 2]>,
    ) {
        store.with_hierarchy(|store, hierarchy| {
            let event = RawEvent {
                diff,
                local: true,
                old_version,
                new_version,
                container_id: self.id.clone(),
            };

            hierarchy.notify(event, &store.reg);
        });
    }

    fn update_hierarchy_on_delete(&mut self, hierarchy: &mut Hierarchy, pos: usize, len: usize) {
        if !hierarchy.has_children(&self.id) {
            return;
        }

        for state in self.state.iter_range(pos, Some(pos + len)) {
            let range = &state.as_ref().0;
            for value in self.raw_data.slice(range).iter() {
                if let LoroValue::Unresolved(container_id) = value {
                    hierarchy.remove_child(&self.id, container_id);
                }
            }
        }
    }

    pub fn values_len(&self) -> usize {
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
                if v.as_unresolved().map(|x| &**x == child).unwrap_or(false) {
                    return Some(Index::Seq(idx));
                }

                idx += 1;
            }
        }

        None
    }

    fn update_hierarchy_on_insert(&mut self, hierarchy: &mut Hierarchy, content: &SliceRange) {
        for value in self.raw_data.slice(&content.0).iter() {
            if let LoroValue::Unresolved(container_id) = value {
                hierarchy.add_child(&self.id, container_id);
            }
        }
    }

    #[cfg(feature = "json")]
    pub fn to_json(&self, reg: &ContainerRegistry) -> LoroValue {
        let mut arr = Vec::new();
        for i in 0..self.values_len() {
            let v = self.get(i).unwrap();
            arr.push(v.to_json_value(reg));
        }
        arr.into()
    }
}

impl Container for ListContainer {
    fn id(&self) -> &ContainerID {
        &self.id
    }

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
                    let data = self.raw_data.slice(&slice.0);
                    smallvec::smallvec![RemoteContent::List(ListOp::Insert {
                        pos,
                        slice: ListSlice::RawData(data.to_vec()),
                    })]
                }
                InnerListOp::Delete(del) => {
                    smallvec::smallvec![RemoteContent::List(ListOp::Delete(del))]
                }
            },
            InnerContent::Map(_) => unreachable!(),
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
                        let mut delta = Delta::new();
                        let delta_vec = self.raw_data.slice(&slice.0).to_vec();
                        delta.retain(*pos);
                        delta.insert(delta_vec);
                        context
                            .diff
                            .entry(self.id.clone())
                            .or_default()
                            .push(Diff::List(delta));
                    }

                    self.update_hierarchy_on_insert(hierarchy, slice);
                    self.state.insert(*pos, slice.clone());
                }
                InnerListOp::Delete(span) => {
                    if should_notify {
                        let mut delta = Delta::new();
                        delta.retain(span.start() as usize);
                        delta.delete(span.atom_len());
                        context
                            .diff
                            .entry(self.id.clone())
                            .or_default()
                            .push(Diff::List(delta));
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

    fn track_retreat(&mut self, spans: &IdSpanVector) {
        self.tracker.retreat(spans);
    }

    fn track_forward(&mut self, spans: &IdSpanVector) {
        self.tracker.forward(spans);
    }

    fn tracker_checkout(&mut self, vv: &crate::VersionVector) {
        if (!vv.is_empty() || self.tracker.start_vv().is_empty())
            && self.tracker.all_vv() >= vv
            && vv >= self.tracker.start_vv()
        {
            self.tracker.checkout(vv);
        } else {
            self.tracker = Tracker::new(vv.clone(), Counter::MAX / 2);
        }
    }

    fn track_apply(
        &mut self,
        _: &mut Hierarchy,
        rich_op: &RichOp,
        import_context: &mut ImportContext,
    ) {
        self.tracker.track_apply(rich_op);
    }

    fn apply_tracked_effects_from(
        &mut self,
        store: &mut LogStore,
        import_context: &mut ImportContext,
    ) {
        let should_notify = store.hierarchy.should_notify(&self.id);
        let mut diff = vec![];
        for effect in self
            .tracker
            .iter_effects(&import_context.old_vv, &import_context.spans)
        {
            match effect {
                Effect::Del { pos, len } => {
                    if should_notify {
                        let mut delta = Delta::new();
                        delta.retain(pos);
                        delta.delete(len);
                        diff.push(Diff::List(delta));
                    }

                    if store.hierarchy.has_children(&self.id) {
                        for state in self.state.iter_range(pos, Some(pos + len)) {
                            let range = &state.as_ref().0;
                            for value in self.raw_data.slice(range).iter() {
                                if let LoroValue::Unresolved(container_id) = value {
                                    store.hierarchy.remove_child(&self.id, container_id);
                                }
                            }
                        }
                    }

                    self.state.delete_range(Some(pos), Some(pos + len));
                }
                Effect::Ins { pos, content } => {
                    if should_notify {
                        let mut delta_vec = vec![];
                        for value in self.raw_data.slice(&content.0) {
                            delta_vec.push(value.clone());
                        }
                        let mut delta = Delta::new();
                        delta.retain(pos);
                        delta.insert(delta_vec);
                        diff.push(Diff::List(delta));
                    }
                    {
                        let content = &content;
                        for value in self.raw_data.slice(&content.0).iter() {
                            if let LoroValue::Unresolved(container_id) = value {
                                store.hierarchy.add_child(&self.id, container_id);
                            }
                        }
                    };

                    self.state.insert(pos, content);
                }
            }
        }

        if should_notify {
            import_context
                .diff
                .entry(self.id.clone())
                .or_default()
                .append(&mut diff);
        }
    }
}

pub struct List {
    instance: Arc<Mutex<ContainerInstance>>,
    client_id: ClientID,
}

impl Clone for List {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
            client_id: self.client_id,
        }
    }
}

impl List {
    pub fn from_instance(instance: Arc<Mutex<ContainerInstance>>, client_id: ClientID) -> Self {
        Self {
            instance,
            client_id,
        }
    }

    pub fn insert_batch<C: Context>(
        &mut self,
        ctx: &C,
        pos: usize,
        values: Vec<LoroValue>,
    ) -> Result<(), LoroError> {
        self.with_container_checked(ctx, |x| x.insert_batch(ctx, pos, values))
    }

    pub fn insert<C: Context, P: Prelim>(
        &mut self,
        ctx: &C,
        pos: usize,
        value: P,
    ) -> Result<Option<ContainerID>, LoroError> {
        self.with_container_checked(ctx, |x| x.insert(ctx, pos, value))
    }

    pub fn delete<C: Context>(
        &mut self,
        ctx: &C,
        pos: usize,
        len: usize,
    ) -> Result<Option<ID>, LoroError> {
        self.with_container_checked(ctx, |list| list.delete(ctx, pos, len))
    }

    pub fn get(&self, pos: usize) -> Option<LoroValue> {
        self.with_container(|list| list.get(pos))
    }

    pub fn len(&self) -> usize {
        self.with_container(|text| text.values_len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ContainerWrapper for List {
    type Container = ListContainer;

    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let mut container_instance = self.instance.lock().unwrap();
        let list = container_instance.as_list_mut().unwrap();
        f(list)
    }

    fn client_id(&self) -> ClientID {
        self.client_id
    }
}
