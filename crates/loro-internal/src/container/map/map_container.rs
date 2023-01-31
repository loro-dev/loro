use std::sync::{Mutex, Weak};

use super::{super::pool::Pool, InnerMapSet};
use crate::{
    container::{
        pool_mapping::{MapPoolMapping, StateContent},
        registry::ContainerRegistry,
    },
    op::OwnedRichOp,
    LoroError,
};
use fxhash::FxHashMap;
use smallvec::{smallvec, SmallVec};

use crate::{
    container::{
        registry::{ContainerInstance, ContainerWrapper},
        Container, ContainerID, ContainerType,
    },
    context::Context,
    event::{Diff, Index, MapDiff, RawEvent},
    hierarchy::Hierarchy,
    id::ClientID,
    log_store::ImportContext,
    op::{InnerContent, Op, RemoteContent, RichOp},
    prelim::Prelim,
    span::HasLamport,
    value::LoroValue,
    version::{Frontiers, TotalOrderStamp},
    InternalString,
};

use super::MapSet;

/// We can only insert to Map
/// delete = set null
///
#[derive(Debug)]
pub struct MapContainer {
    id: ContainerID,
    pub(crate) state: FxHashMap<InternalString, ValueSlot>,
    pub(crate) pool: Pool,
    pending_ops: Vec<OwnedRichOp>,
    pool_mapping: Option<MapPoolMapping>,
}

#[derive(Debug, Clone, Copy)]
pub struct ValueSlot {
    pub(crate) value: u32,
    pub(crate) order: TotalOrderStamp,
}

// FIXME: make map container support checkout to certain version
impl MapContainer {
    #[inline]
    pub(crate) fn new(id: ContainerID) -> Self {
        MapContainer {
            id,
            state: FxHashMap::default(),
            pool: Pool::default(),
            pending_ops: Vec::new(),
            pool_mapping: None,
        }
    }

    pub fn insert<C: Context, P: Prelim>(
        &mut self,
        ctx: &C,
        key: InternalString,
        value: P,
    ) -> Result<(Option<RawEvent>, Option<ContainerID>), LoroError> {
        let (value, maybe_container) = value.convert_value()?;
        if let Some(prelim) = maybe_container {
            let (event, container_id) = self.insert_obj(ctx, key, value.into_container().unwrap());
            let m = ctx.log_store();
            let store = m.read().unwrap();
            let container = store.get_container(&container_id).unwrap();
            drop(store);
            prelim.integrate(ctx, container)?;
            Ok((event, Some(container_id)))
        } else {
            let value = value.into_value().unwrap();
            let event = self.insert_value(ctx, key, value)?;
            Ok((event, None))
        }
    }

    fn insert_value<C: Context>(
        &mut self,
        ctx: &C,
        key: InternalString,
        value: LoroValue,
    ) -> Result<Option<RawEvent>, LoroError> {
        assert!(value.as_unresolved().is_none(), "To insert a container to map, you should use insert_obj method or insert with a Prelim container value");
        let value_index = self.pool.alloc(value).start;
        let new_value_idx = value_index;
        let self_id = &self.id;
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
        let hierarchy = ctx.hierarchy();
        let client_id = store.this_client_id;
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        let id = store.next_id_for(client_id);
        let container = store.get_container_idx(self_id).unwrap();
        let old_version: Frontiers = store.frontiers().iter().copied().collect();
        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: InnerContent::Map(InnerMapSet {
                key: key.clone(),
                value: new_value_idx,
            }),
        }]);
        let new_version: Frontiers = store.frontiers().iter().copied().collect();
        let mut h = hierarchy.try_lock().unwrap();
        self.update_hierarchy_if_container_is_overwritten(&key, &mut h);

        let ans = if h.should_notify(self.id()) {
            let diff = vec![Diff::Map(calculate_map_diff(self, &key, new_value_idx))];
            h.get_abs_path(&store.reg, self.id()).map(|x| RawEvent {
                diff,
                container_id: self.id.clone(),
                old_version,
                new_version,
                local: true,
                abs_path: x,
            })
        } else {
            None
        };

        self.state.insert(
            key,
            ValueSlot {
                value: new_value_idx,
                order,
            },
        );

        Ok(ans)
    }

    fn insert_obj<C: Context>(
        &mut self,
        ctx: &C,
        key: InternalString,
        obj: ContainerType,
    ) -> (Option<RawEvent>, ContainerID) {
        debug_log::debug_log!("Insert Obj");
        let self_id = &self.id;
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
        let hierarchy = ctx.hierarchy();
        let client_id = store.this_client_id;
        let (container_id, _) = store.create_container(obj);
        let value = self.pool.alloc(container_id.clone()).start;
        let id = store.next_id_for(client_id);
        let self_idx = store.get_container_idx(self_id).unwrap();
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        let old_version: Frontiers = store.frontiers().iter().copied().collect();
        store.append_local_ops(&[Op {
            counter: id.counter,
            container: self_idx,
            content: InnerContent::Map(InnerMapSet {
                value,
                key: key.clone(),
            }),
        }]);
        let new_version: Frontiers = store.frontiers().iter().copied().collect();
        let mut hierarchy = hierarchy.try_lock().unwrap();
        hierarchy.add_child(&self.id, &container_id);
        self.update_hierarchy_if_container_is_overwritten(&key, &mut hierarchy);

        let ans = if hierarchy.should_notify(self.id()) {
            debug_log::debug_log!("SHOULD NOTIFY");
            let diff = calculate_map_diff(self, &key, value);
            (
                hierarchy
                    .get_abs_path(&store.reg, self.id())
                    .map(|x| RawEvent {
                        container_id: self.id.clone(),
                        old_version,
                        new_version,
                        local: true,
                        diff: vec![Diff::Map(diff)],
                        abs_path: x,
                    }),
                container_id,
            )
        } else {
            debug_log::debug_log!("no NOTIFY");
            (None, container_id)
        };
        self.state.insert(key, ValueSlot { value, order });
        ans
    }

    fn update_hierarchy_if_container_is_overwritten(
        &mut self,
        key: &InternalString,
        h: &mut Hierarchy,
    ) {
        if let Some(old_value) = self.state.get(key) {
            let v = &self.pool[old_value.value];
            if let Some(container) = v.as_unresolved() {
                h.remove_child(&self.id, container);
            }
        }
    }

    #[inline]
    pub fn delete<C: Context>(
        &mut self,
        ctx: &C,
        key: InternalString,
    ) -> Result<Option<RawEvent>, LoroError> {
        self.insert_value(ctx, key, LoroValue::Null)
    }

    pub fn index_of_child(&self, child: &ContainerID) -> Option<Index> {
        for (key, value) in self.state.iter() {
            if self.pool[value.value]
                .as_unresolved()
                .map(|x| &**x == child)
                .unwrap_or(false)
            {
                return Some(Index::Key(key.clone()));
            }
        }

        None
    }

    #[inline]
    pub fn get(&self, key: &InternalString) -> Option<&LoroValue> {
        self.state
            .get(key)
            .map(|v| self.pool.slice(&(v.value..v.value + 1)).first().unwrap())
    }

    pub fn to_json(&self, reg: &ContainerRegistry) -> LoroValue {
        self.get_value().resolve_deep(reg)
    }

    pub fn keys(&self) -> Vec<InternalString> {
        self.state.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<LoroValue> {
        self.state
            .values()
            .map(|value| {
                let index = value.value;
                let value = self.pool.slice(&(index..index + 1))[0].clone();
                value
                // if let Some(container_id) = value.as_unresolved() {
                //     LoroValue::Unresolved(container_id.clone())
                // } else {
                //      value
                // }
            })
            .collect()
    }
}

fn calculate_map_diff(
    this: &mut MapContainer,
    key: &InternalString,
    new_value_idx: u32,
) -> MapDiff {
    let mut diff = MapDiff::default();
    let old_value = this.get(key);
    let new_value = &this.pool[new_value_idx];
    match old_value {
        Some(old) => {
            diff.updated
                .insert(key.clone(), (old.clone(), new_value.clone()).into());
        }
        None => {
            diff.added.insert(key.clone(), new_value.clone());
        }
    }
    diff
}

impl Container for MapContainer {
    #[inline(always)]
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn type_(&self) -> ContainerType {
        ContainerType::Map
    }

    fn get_value(&self) -> LoroValue {
        let mut map = FxHashMap::default();
        for (key, value) in self.state.iter() {
            let index = value.value;
            let value = self.pool.slice(&(index..index + 1))[0].clone();
            if let Some(container_id) = value.as_unresolved() {
                map.insert(
                    key.to_string(),
                    // TODO: make a from
                    LoroValue::Unresolved(container_id.clone()),
                );
            } else {
                map.insert(key.to_string(), value);
            }
        }

        map.into()
    }

    fn tracker_init(&mut self, _vv: &crate::version::PatchedVersionVector) {}

    fn tracker_checkout(&mut self, _vv: &crate::version::PatchedVersionVector) {}

    fn to_export(&mut self, content: InnerContent, _gc: bool) -> SmallVec<[RemoteContent; 1]> {
        if let Ok(set) = content.into_map() {
            let index = set.value;
            let value = self.pool.slice(&(index..index + 1))[0].clone();
            return smallvec![RemoteContent::Map(MapSet {
                key: set.key,
                value,
            })];
        }

        unreachable!()
    }

    fn to_import(&mut self, mut content: RemoteContent) -> InnerContent {
        if let Some(set) = content.as_map_mut() {
            let index = self.pool.alloc(std::mem::take(&mut set.value));
            return InnerContent::Map(InnerMapSet {
                key: set.key.clone(),
                value: index.start,
            });
        }
        unreachable!()
    }

    // TODO: refactor
    fn update_state_directly(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        ctx: &mut ImportContext,
    ) {
        let content = op.get_sliced().content;
        let new_val: &InnerMapSet = content.as_map().unwrap();
        let order = TotalOrderStamp {
            lamport: op.lamport(),
            client_id: op.client_id(),
        };
        let should_notify = hierarchy.should_notify(&self.id);
        if let Some(slot) = self.state.get_mut(&new_val.key) {
            if slot.order < order {
                let new_value = &self.pool[new_val.value];
                if should_notify {
                    let mut map_diff = MapDiff::default();
                    map_diff.updated.insert(
                        new_val.key.clone(),
                        (self.pool[slot.value].clone(), new_value.clone()).into(),
                    );
                    ctx.push_diff(&self.id, Diff::Map(map_diff));
                }

                let old_val = &self.pool[slot.value];
                if let Some(container) = old_val.as_unresolved() {
                    hierarchy.remove_child(&self.id, container);
                }
                if let Some(container) = new_value.as_unresolved() {
                    hierarchy.add_child(&self.id, container);
                }

                slot.value = new_val.value;
                slot.order = order;
            }
        } else {
            let new_value = &self.pool[new_val.value];
            if should_notify {
                let mut map_diff = MapDiff::default();
                map_diff
                    .added
                    .insert(new_val.key.clone(), self.pool[new_val.value].clone());
                ctx.push_diff(&self.id, Diff::Map(map_diff));
            }

            if let Some(container) = new_value.as_unresolved() {
                hierarchy.add_child(&self.id, container);
            }

            self.state.insert(
                new_val.key.to_owned(),
                ValueSlot {
                    value: new_val.value,
                    order,
                },
            );
        }
    }

    fn track_apply(&mut self, _: &mut Hierarchy, op: &RichOp, _: &mut ImportContext) {
        self.pending_ops.push(op.as_owned());
    }

    fn apply_tracked_effects_from(
        &mut self,
        hierarchy: &mut Hierarchy,
        import_context: &mut ImportContext,
    ) {
        for op in std::mem::take(&mut self.pending_ops) {
            self.update_state_directly(hierarchy, &op.rich_op(), import_context)
        }
    }

    fn initialize_pool_mapping(&mut self) {
        let mut pool_mapping = MapPoolMapping::default();
        for value in self.state.values() {
            let index = value.value;
            pool_mapping.push_state_slice(index, &self.pool.slice(&(index..index + 1))[0]);
        }
        self.pool_mapping = Some(pool_mapping);
    }

    fn encode_and_release_pool_mapping(&mut self) -> StateContent {
        let pool_mapping = self.pool_mapping.take().unwrap();
        let (keys, values) = self
            .state
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    ValueSlot {
                        value: pool_mapping.get_new_index(v.value),
                        order: v.order,
                    },
                )
            })
            .unzip();
        StateContent::Map {
            pool: pool_mapping.inner(),
            keys,
            values,
        }
    }

    fn to_export_snapshot(
        &mut self,
        content: &InnerContent,
        _gc: bool,
    ) -> SmallVec<[InnerContent; 1]> {
        match content {
            InnerContent::Map(set) => {
                let index = set.value;
                let value = self
                    .pool_mapping
                    .as_mut()
                    .unwrap()
                    .convert_ops_value(index, &self.pool[index]);
                smallvec![InnerContent::Map(InnerMapSet {
                    key: set.key.clone(),
                    value,
                })]
            }
            _ => unreachable!(),
        }
    }

    fn to_import_snapshot(
        &mut self,
        state_content: StateContent,
        hierarchy: &mut Hierarchy,
        ctx: &mut ImportContext,
    ) {
        if let StateContent::Map { pool, keys, values } = state_content {
            for v in pool.iter() {
                if let LoroValue::Unresolved(child_container_id) = v {
                    hierarchy.add_child(self.id(), child_container_id.as_ref());
                }
            }
            self.pool = pool.into();
            self.state = keys.into_iter().zip(values).collect();
            // notify
            let should_notify = hierarchy.should_notify(&self.id);
            if should_notify {
                let mut map_diff = MapDiff::default();
                for (k, v) in self.state.iter() {
                    map_diff.added.insert(k.clone(), self.pool[v.value].clone());
                }
                ctx.push_diff(&self.id, Diff::Map(map_diff));
            }
        } else {
            unreachable!()
        }
    }
}

pub struct Map {
    instance: Weak<Mutex<ContainerInstance>>,
    client_id: ClientID,
}

impl Clone for Map {
    fn clone(&self) -> Self {
        Self {
            instance: Weak::clone(&self.instance),
            client_id: self.client_id,
        }
    }
}

impl Map {
    pub fn from_instance(instance: Weak<Mutex<ContainerInstance>>, client_id: ClientID) -> Self {
        Self {
            instance,
            client_id,
        }
    }

    pub fn insert<C: Context, V: Prelim>(
        &mut self,
        ctx: &C,
        key: &str,
        value: V,
    ) -> Result<Option<ContainerID>, LoroError> {
        self.with_event(ctx, |map| map.insert(ctx, key.into(), value))
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, key: &str) -> Result<(), LoroError> {
        self.with_event(ctx, |map| Ok((map.delete(ctx, key.into())?, ())))
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.with_container(|map| map.get(&key.into()).cloned())
    }

    pub fn keys(&self) -> Vec<String> {
        self.with_container(|map| map.keys().into_iter().map(|k| k.to_string()).collect())
    }

    pub fn values(&self) -> Vec<LoroValue> {
        self.with_container(|map| map.values())
    }

    pub fn for_each<F>(&self, f: F)
    where
        F: Fn(&InternalString, &LoroValue),
    {
        self.with_container(|map| {
            for (k, v) in map.state.iter() {
                let value = &map.pool.slice(&(v.value..v.value + 1))[0];
                f(k, value);
            }
        })
    }

    pub fn id(&self) -> ContainerID {
        self.instance
            .upgrade()
            .unwrap()
            .try_lock()
            .unwrap()
            .as_map()
            .unwrap()
            .id
            .clone()
    }

    pub fn get_value(&self) -> LoroValue {
        self.instance
            .upgrade()
            .unwrap()
            .try_lock()
            .unwrap()
            .as_map()
            .unwrap()
            .get_value()
    }

    pub fn len(&self) -> usize {
        self.with_container(|map| map.state.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ContainerWrapper for Map {
    type Container = MapContainer;

    #[inline(always)]
    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let w = self.instance.upgrade().unwrap();
        let mut container_instance = w.try_lock().unwrap();
        let map = container_instance.as_map_mut().unwrap();
        let ans = f(map);
        drop(container_instance);
        ans
    }

    fn client_id(&self) -> ClientID {
        self.client_id
    }
}
