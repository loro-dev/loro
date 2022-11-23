use std::sync::{Arc, Mutex};

use super::{super::pool::Pool, InnerMapSet};
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
    version::{Frontiers, IdSpanVector, TotalOrderStamp},
    InternalString, LogStore,
};

use super::MapSet;

/// We can only insert to Map
/// delete = set null
///
#[derive(Debug)]
pub struct MapContainer {
    id: ContainerID,
    state: FxHashMap<InternalString, ValueSlot>,
    pool: Pool,
}

#[derive(Debug)]
struct ValueSlot {
    value: u32,
    order: TotalOrderStamp,
}

// FIXME: make map container support checkout to certain version
impl MapContainer {
    #[inline]
    pub(crate) fn new(id: ContainerID) -> Self {
        MapContainer {
            id,
            state: FxHashMap::default(),
            pool: Pool::default(),
        }
    }

    pub fn insert<C: Context, P: Prelim>(
        &mut self,
        ctx: &C,
        key: InternalString,
        value: P,
    ) -> Option<ContainerID> {
        let (value, maybe_container) = value.convert_value();
        if let Some(prelim) = maybe_container {
            let container_id = self.insert_obj(ctx, key, value.into_container().unwrap());
            let m = ctx.log_store();
            let store = m.read().unwrap();
            let container = Arc::clone(store.get_container(&container_id).unwrap());
            drop(store);
            prelim.integrate(ctx, &container);
            Some(container_id)
        } else {
            let value = value.into_value().unwrap();
            self.insert_value(ctx, key, value);
            None
        }
    }

    fn insert_value<C: Context>(&mut self, ctx: &C, key: InternalString, value: LoroValue) {
        assert!(value.as_unresolved().is_none(), "To insert a container to map, you should use insert_obj method or insert with a Prelim container value");
        let value_index = self.pool.alloc(value).start;
        let new_value_idx = value_index;
        let self_id = &self.id;
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
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
        self.notify_local(&mut store, old_version, new_version, |this| {
            vec![Diff::Map(calculate_map_diff(this, &key, new_value_idx))]
        });

        self.update_hierarchy_if_container_is_overwritten(&key, &mut store);
        self.state.insert(
            key,
            ValueSlot {
                value: new_value_idx,
                order,
            },
        );
    }

    fn insert_obj<C: Context>(
        &mut self,
        ctx: &C,
        key: InternalString,
        obj: ContainerType,
    ) -> ContainerID {
        let self_id = &self.id;
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
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
        self.notify_local(&mut store, old_version, new_version, |this| {
            let diff = calculate_map_diff(this, &key, value);
            vec![Diff::Map(diff)]
        });

        store.hierarchy.add_child(&self.id, &container_id);
        self.update_hierarchy_if_container_is_overwritten(&key, &mut store);

        self.state.insert(key, ValueSlot { value, order });
        container_id
    }

    #[inline(always)]
    fn notify_local<F>(
        &mut self,
        store: &mut LogStore,
        old_version: Frontiers,
        new_version: Frontiers,
        get_diff: F,
    ) where
        F: FnOnce(&mut Self) -> Vec<Diff>,
    {
        if store.hierarchy.should_notify(&self.id) {
            store.with_hierarchy(|store, hierarchy| {
                let event = RawEvent {
                    diff: get_diff(self),
                    local: true,
                    old_version,
                    new_version,
                    container_id: self.id.clone(),
                };

                hierarchy.notify(event, &store.reg);
            });
        }
    }

    fn update_hierarchy_if_container_is_overwritten(
        &mut self,
        key: &InternalString,
        store: &mut LogStore,
    ) {
        if let Some(old_value) = self.state.get(key) {
            let v = &self.pool[old_value.value];
            if let Some(container) = v.as_unresolved() {
                store.hierarchy.remove_child(&self.id, container);
            }
        }
    }

    #[inline]
    pub fn delete<C: Context>(&mut self, ctx: &C, key: InternalString) {
        self.insert_value(ctx, key, LoroValue::Null);
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

    #[cfg(feature = "json")]
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (k, v) in self.state.iter() {
            let value = self.pool.slice(&(v.value..v.value + 1)).first().unwrap();
            map.insert(k.to_string(), value.to_json_value());
        }
        serde_json::Value::Object(map)
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

    fn tracker_checkout(&mut self, _vv: &crate::version::VersionVector) {}

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
                    ctx.diff
                        .entry(self.id.clone())
                        .or_default()
                        .push(Diff::Map(map_diff));
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
                ctx.diff
                    .entry(self.id.clone())
                    .or_default()
                    .push(Diff::Map(map_diff));
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

    fn track_retreat(&mut self, _: &IdSpanVector) {}

    fn track_forward(&mut self, _: &IdSpanVector) {}

    fn track_apply(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        import_context: &mut ImportContext,
    ) {
        self.update_state_directly(hierarchy, op, import_context);
    }

    fn apply_tracked_effects_from(
        &mut self,
        _store: &mut crate::LogStore,
        _import_context: &mut ImportContext,
    ) {
    }
}

pub struct Map {
    instance: Arc<Mutex<ContainerInstance>>,
    client_id: ClientID,
}

impl Clone for Map {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
            client_id: self.client_id,
        }
    }
}

impl Map {
    pub fn from_instance(instance: Arc<Mutex<ContainerInstance>>, client_id: ClientID) -> Self {
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
    ) -> Result<Option<ContainerID>, crate::LoroError> {
        self.with_container_checked(ctx, |map| map.insert(ctx, key.into(), value))
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, key: &str) -> Result<(), crate::LoroError> {
        self.with_container_checked(ctx, |map| {
            map.delete(ctx, key.into());
        })
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.with_container(|map| map.get(&key.into()).cloned())
    }

    pub fn id(&self) -> ContainerID {
        self.instance.lock().unwrap().as_map().unwrap().id.clone()
    }

    pub fn get_value(&self) -> LoroValue {
        self.instance.lock().unwrap().as_map().unwrap().get_value()
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
        let mut container_instance = self.instance.lock().unwrap();
        let map = container_instance.as_map_mut().unwrap();
        f(map)
    }

    fn client_id(&self) -> ClientID {
        self.client_id
    }
}
