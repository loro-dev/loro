use std::sync::{Arc, Mutex};

use rle::HasLength;
use smallvec::SmallVec;
use tracing::instrument;

use crate::{
    container::{
        list::list_op::{InnerListOp, ListOp},
        registry::{ContainerInstance, ContainerWrapper},
        Container, ContainerID, ContainerType,
    },
    context::Context,
    delta::Delta,
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::{ClientID, Counter},
    log_store::ImportContext,
    op::{InnerContent, Op, RemoteContent, RichOp},
    value::LoroValue,
    version::PatchedVersionVector,
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
    state: Rope,
    raw_str: StringPool,
    tracker: Option<Tracker>,
}

impl TextContainer {
    pub(crate) fn new(id: ContainerID) -> Self {
        Self {
            id,
            raw_str: StringPool::default(),
            tracker: None,
            state: Default::default(),
        }
    }

    #[instrument(skip_all)]
    pub fn insert<C: Context>(&mut self, ctx: &C, pos: usize, text: &str) -> Option<RawEvent> {
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
        let op_slice = SliceRange::from_pool_string(&slice);
        let range = slice.clone();
        self.state.insert(pos, slice);
        let op = Op::new(
            id,
            InnerContent::List(InnerListOp::Insert {
                slice: op_slice,
                pos,
            }),
            store.get_or_create_container_idx(&self.id),
        );

        let (old_version, new_version) = store.append_local_ops(&[op]);
        let new_version = new_version.into();

        // notify
        let h = store.hierarchy.clone();
        let h = h.try_lock().unwrap();
        if h.should_notify(&self.id) {
            let mut delta = Delta::new();
            delta.retain(pos);
            delta.insert(text.to_owned());
            h.get_abs_path(&store.reg, self.id())
                .map(|abs_path| RawEvent {
                    diff: vec![Diff::Text(delta)],
                    local: true,
                    old_version,
                    new_version,
                    container_id: self.id.clone(),
                    abs_path,
                })
        } else {
            None
        }
    }

    #[instrument(skip_all)]
    pub fn delete<C: Context>(&mut self, ctx: &C, pos: usize, len: usize) -> Option<RawEvent> {
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

        // notify
        let h = store.hierarchy.clone();
        let h = h.try_lock().unwrap();
        self.state.delete_range(Some(pos), Some(pos + len));
        if h.should_notify(&self.id) {
            let mut delta = Delta::new();
            delta.retain(pos);
            delta.delete(len);
            h.get_abs_path(&store.reg, self.id())
                .map(|abs_path| RawEvent {
                    diff: vec![Diff::Text(delta)],
                    local: true,
                    old_version,
                    new_version,
                    container_id: self.id.clone(),
                    abs_path,
                })
        } else {
            None
        }
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

impl Container for TextContainer {
    fn id(&self) -> &ContainerID {
        &self.id
    }

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
                        let mut delta = Delta::new();
                        delta.retain(*pos);
                        delta.insert(s);
                        ctx.push_diff(&self.id, Diff::Text(delta));
                    }
                    self.state.insert(
                        *pos,
                        PoolString::from_slice_range(&self.raw_str, slice.clone()),
                    );
                }
                InnerListOp::Delete(span) => {
                    if should_notify {
                        let mut delta = Delta::new();
                        delta.retain(span.start() as usize);
                        delta.delete(span.atom_len());
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
        let mut diff = vec![];
        for effect in self.tracker.as_mut().unwrap().iter_effects(
            import_context.patched_old_vv.as_ref().unwrap(),
            &import_context.spans,
        ) {
            match effect {
                Effect::Del { pos, len } => {
                    if should_notify {
                        let mut delta = Delta::new();
                        delta.retain(pos);
                        delta.delete(len);
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
                        let mut delta = Delta::new();
                        delta.retain(pos);
                        delta.insert(s);
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
}

pub struct Text {
    instance: Arc<Mutex<ContainerInstance>>,
    client_id: ClientID,
}

impl Clone for Text {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
            client_id: self.client_id,
        }
    }
}

impl Text {
    pub fn from_instance(instance: Arc<Mutex<ContainerInstance>>, client_id: ClientID) -> Self {
        Self {
            instance,
            client_id,
        }
    }

    pub fn id(&self) -> ContainerID {
        self.instance.lock().unwrap().as_text().unwrap().id.clone()
    }

    pub fn insert<C: Context>(
        &mut self,
        ctx: &C,
        pos: usize,
        text: &str,
    ) -> Result<(), crate::LoroError> {
        self.with_event(ctx, |x| (x.insert(ctx, pos, text), ()))
    }

    pub fn delete<C: Context>(
        &mut self,
        ctx: &C,
        pos: usize,
        len: usize,
    ) -> Result<(), crate::LoroError> {
        self.with_event(ctx, |text| (text.delete(ctx, pos, len), ()))
    }

    pub fn get_value(&self) -> LoroValue {
        self.instance.lock().unwrap().as_text().unwrap().get_value()
    }

    pub fn len(&self) -> usize {
        self.with_container(|text| text.text_len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ContainerWrapper for Text {
    type Container = TextContainer;

    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let mut container_instance = self.instance.lock().unwrap();
        let text = container_instance.as_text_mut().unwrap();
        let ans = f(text);
        drop(container_instance);
        ans
    }

    fn client_id(&self) -> crate::id::ClientID {
        self.client_id
    }
}
