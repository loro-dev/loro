// TODO: refactor, extract common code with text
use std::sync::{Arc, Mutex};

use rle::{
    rle_tree::{tree_trait::CumulateTreeTrait, HeapMode},
    RleTree,
};
use smallvec::SmallVec;

use crate::{
    container::{
        list::list_op::ListOp,
        pool,
        registry::{ContainerInstance, ContainerWrapper},
        text::{
            text_content::{ListSlice, SliceRange},
            tracker::{Effect, Tracker},
        },
        Container, ContainerID, ContainerType,
    },
    context::Context,
    id::{Counter, ID},
    op::{InnerContent, Op, RemoteContent, RichOp},
    value::LoroValue,
    version::IdSpanVector,
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

        let store = ctx.log_store();
        let mut store = store.write().unwrap();
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
            InnerContent::List(InnerListOp::Insert {
                slice: slice.into(),
                pos,
            }),
            store.get_or_create_container_idx(&self.id),
        );
        store.append_local_ops(&[op]);

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
            InnerContent::List(InnerListOp::new_del(pos, len)),
            store.get_or_create_container_idx(&self.id),
        );

        store.append_local_ops(&[op]);
        self.state.delete_range(Some(pos), Some(pos + len));
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
        let container_id = store.create_container(obj);
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

    fn update_state_directly(&mut self, op: &RichOp) {
        match &op.get_sliced().content {
            InnerContent::List(op) => match op {
                InnerListOp::Insert { slice, pos } => self.state.insert(*pos, slice.clone()),
                InnerListOp::Delete(span) => self
                    .state
                    .delete_range(Some(span.start() as usize), Some(span.end() as usize)),
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

    fn track_apply(&mut self, rich_op: &RichOp) {
        self.tracker.track_apply(rich_op);
    }

    fn apply_tracked_effects_from(
        &mut self,
        from: &crate::VersionVector,
        effect_spans: &IdSpanVector,
    ) {
        for effect in self.tracker.iter_effects(from, effect_spans) {
            match effect {
                Effect::Del { pos, len } => self.state.delete_range(Some(pos), Some(pos + len)),
                Effect::Ins { pos, content } => self.state.insert(pos, content),
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
}

impl From<Arc<Mutex<ContainerInstance>>> for List {
    fn from(text: Arc<Mutex<ContainerInstance>>) -> Self {
        List { instance: text }
    }
}
