// TODO: refactor, extract common code with text
use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use rle::{
    rle_tree::{tree_trait::CumulateTreeTrait, HeapMode},
    HasLength, RleTree, Sliceable,
};
use smallvec::{smallvec, SmallVec};

use crate::{
    container::{
        list::list_op::ListOp,
        registry::{ContainerInstance, ContainerWrapper},
        text::{
            text_content::{ListSlice, SliceRange},
            tracker::{Effect, Tracker},
        },
        Container, ContainerID, ContainerType,
    },
    context::Context,
    id::{Counter, ID},
    op::{Content, Op, RemoteOp, RichOp},
    span::{HasCounterSpan, HasId},
    value::LoroValue,
    version::IdSpanVector,
};

#[derive(Debug)]
pub struct ListContainer {
    id: ContainerID,
    state: RleTree<SliceRange, CumulateTreeTrait<SliceRange, 8, HeapMode>>,
    raw_data: Pool,
    tracker: Tracker,
    head: SmallVec<[ID; 2]>,
}

#[derive(Debug, Default)]
struct Pool(Vec<LoroValue>);

impl Pool {
    #[inline(always)]
    pub fn alloc<V: Into<LoroValue>>(&mut self, s: V) -> Range<u32> {
        self.0.push(s.into());
        (self.0.len() - 1) as u32..self.0.len() as u32
    }

    #[inline(always)]
    pub fn alloc_arr(&mut self, values: Vec<LoroValue>) -> Range<u32> {
        let start = self.0.len() as u32;
        for v in values {
            self.0.push(v);
        }
        start..self.0.len() as u32
    }

    #[inline(always)]
    pub fn slice(&self, range: &Range<u32>) -> &[LoroValue] {
        &self.0[range.start as usize..range.end as usize]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl ListContainer {
    pub(crate) fn new(id: ContainerID) -> Self {
        Self {
            id,
            raw_data: Pool::default(),
            tracker: Tracker::new(Default::default(), 0),
            state: Default::default(),
            // TODO: should be eq to log_store frontier?
            head: Default::default(),
        }
    }

    pub fn insert_batch<C: Context>(&mut self, ctx: &C, pos: usize, values: Vec<LoroValue>) {
        if values.is_empty() {
            return;
        }

        let store = ctx.log_store();
        let mut store = store.write().unwrap();
        let id = store.next_id();
        let slice = self.raw_data.alloc_arr(values);
        self.state.insert(pos, slice.clone().into());
        let op = Op::new(
            id,
            Content::List(ListOp::Insert {
                slice: slice.into(),
                pos,
            }),
            store.get_or_create_container_idx(&self.id),
        );
        let last_id = ID::new(
            store.this_client_id,
            op.counter + op.atom_len() as Counter - 1,
        );
        store.append_local_ops(&[op]);
        self.head = smallvec![last_id];
    }

    pub fn insert<C: Context, V: Into<LoroValue>>(
        &mut self,
        ctx: &C,
        pos: usize,
        value: V,
    ) -> Option<ID> {
        let store = ctx.log_store();
        let mut store = store.write().unwrap();
        let id = store.next_id();
        let slice = self.raw_data.alloc(value);
        self.state.insert(pos, slice.clone().into());
        let op = Op::new(
            id,
            Content::List(ListOp::Insert {
                slice: slice.into(),
                pos,
            }),
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
            Content::List(ListOp::new_del(pos, len)),
            store.get_or_create_container_idx(&self.id),
        );

        let last_id = ID::new(store.this_client_id, op.ctr_last());
        store.append_local_ops(&[op]);
        self.state.delete_range(Some(pos), Some(pos + len));
        self.head = smallvec![last_id];
        Some(id)
    }

    pub fn insert_obj<C: Context>(
        &mut self,
        ctx: &C,
        pos: usize,
        obj: ContainerType,
    ) -> ContainerID {
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
        let container_id = store.create_container(obj, self.id.clone());
        // TODO: we can avoid this lock
        drop(store);
        self.insert(
            ctx,
            pos,
            LoroValue::Unresolved(Box::new(container_id.clone())),
        );
        container_id
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

    fn to_export(&mut self, op: &mut RemoteOp, _gc: bool) {
        for content in op.contents.iter_mut() {
            if let Some((slice, _pos)) = content.as_list_mut().and_then(|x| x.as_insert_mut()) {
                if let Some(change) = if let ListSlice::Slice(ranges) = slice {
                    Some(self.raw_data.slice(&ranges.0))
                } else {
                    None
                } {
                    *slice = ListSlice::RawData(change.to_vec());
                }
            }
        }
    }

    fn to_import(&mut self, op: &mut RemoteOp) {
        for content in op.contents.iter_mut() {
            if let Some((slice, _pos)) = content.as_list_mut().and_then(|x| x.as_insert_mut()) {
                if let Some(slice_range) = match std::mem::take(slice) {
                    ListSlice::RawData(data) => Some(self.raw_data.alloc_arr(data)),
                    _ => unreachable!(),
                } {
                    *slice = slice_range.into();
                }
            }
        }
    }

    fn update_state_directly(&mut self, op: &RichOp) {
        match &op.get_sliced().content {
            Content::List(op) => match op {
                ListOp::Insert { slice, pos } => {
                    self.state.insert(*pos, slice.as_slice().unwrap().clone())
                }
                ListOp::Delete(span) => self
                    .state
                    .delete_range(Some(span.start() as usize), Some(span.end() as usize)),
            },
            Content::Container(_) => {}
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

    fn track_apply(&mut self, rich_op: &RichOp) {
        let content = rich_op.get_sliced().content;
        if content.as_container().is_some() {
            return;
        }

        let id = rich_op.id_start();
        if self
            .tracker
            .all_vv()
            .includes_id(id.inc(content.atom_len() as Counter - 1))
        {
            self.tracker
                .forward(&id.to_span(content.atom_len()).to_id_span_vec());
            return;
        }

        if self.tracker.all_vv().includes_id(id) {
            let this_ctr = self.tracker.all_vv().get(&id.client_id).unwrap();
            let shift = this_ctr - id.counter;
            self.tracker
                .forward(&id.to_span(shift as usize).to_id_span_vec());
            self.tracker.apply(
                id.inc(shift),
                &content.slice(shift as usize, content.atom_len()),
            );
        } else {
            self.tracker.apply(id, &content)
        }
    }

    fn apply_tracked_effects_from(
        &mut self,
        from: &crate::VersionVector,
        effect_spans: &IdSpanVector,
    ) {
        self.tracker.checkout(from);
        for effect in self.tracker.iter_effects(effect_spans) {
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
        }
    }
}

pub struct List {
    instance: Arc<Mutex<ContainerInstance>>,
}

impl Clone for List {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
        }
    }
}

impl List {
    pub fn insert_batch<C: Context>(&mut self, ctx: &C, pos: usize, values: Vec<LoroValue>) {
        self.with_container(|x| x.insert_batch(ctx, pos, values))
    }

    pub fn insert<C: Context, V: Into<LoroValue>>(
        &mut self,
        ctx: &C,
        pos: usize,
        value: V,
    ) -> Option<ID> {
        self.with_container(|x| x.insert(ctx, pos, value))
    }

    pub fn insert_obj<C: Context>(
        &mut self,
        ctx: &C,
        pos: usize,
        obj: ContainerType,
    ) -> ContainerID {
        self.with_container(|x| x.insert_obj(ctx, pos, obj))
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, pos: usize, len: usize) -> Option<ID> {
        self.with_container(|text| text.delete(ctx, pos, len))
    }

    pub fn values_len(&self) -> usize {
        self.with_container(|text| text.values_len())
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
}

impl From<Arc<Mutex<ContainerInstance>>> for List {
    fn from(text: Arc<Mutex<ContainerInstance>>) -> Self {
        List { instance: text }
    }
}
