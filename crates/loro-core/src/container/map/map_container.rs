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
    id::ClientID,
    op::{InnerContent, Op, RemoteContent, RichOp},
    prelim::Prelim,
    span::HasLamport,
    value::LoroValue,
    version::{IdSpanVector, TotalOrderStamp},
    InternalString,
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
        let ct = value.container_type();
        if let Some(ct) = ct {
            let container_id = self.insert_obj(ctx, key, ct);
            let m = ctx.log_store();
            let store = m.read().unwrap();
            let container = Arc::clone(store.get_container(&container_id).unwrap());
            drop(store);
            value.integrate(ctx, &container);
            Some(container_id)
        } else {
            let value = value.into_loro_value();
            let value_index = self.pool.alloc(value).start;
            let value = value_index;
            self.insert_value(ctx, key, value);
            None
        }
    }

    fn insert_value<C: Context>(&mut self, ctx: &C, key: InternalString, value: u32) {
        let self_id = &self.id;
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
        let client_id = store.this_client_id;
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        let id = store.next_id_for(client_id);
        // // TODO: store this value?
        let container = store.get_container_idx(self_id).unwrap();
        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: InnerContent::Map(InnerMapSet {
                key: key.clone(),
                value,
            }),
        }]);
        self.state.insert(key, ValueSlot { value, order });
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
        let container_id = store.create_container(obj);
        let value_index = self.pool.alloc(container_id.clone()).start;
        let value = value_index;
        // TODO: store this value?
        let id = store.next_id_for(client_id);
        let container = store.get_container_idx(self_id).unwrap();
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: InnerContent::Map(InnerMapSet {
                value,
                key: key.clone(),
            }),
        }]);
        self.state.insert(key, ValueSlot { value, order });
        container_id
    }

    #[inline]
    pub fn delete<C: Context>(&mut self, ctx: &C, key: InternalString) {
        self.insert(ctx, key, LoroValue::Null);
    }

    #[inline]
    pub fn get(&self, key: &InternalString) -> Option<LoroValue> {
        self.state
            .get(key)
            .map(|v| self.pool.slice(&(v.value..v.value + 1)).first().unwrap())
            .cloned()
    }
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

    fn update_state_directly(&mut self, op: &RichOp) {
        let content = op.get_sliced().content;
        let v: &InnerMapSet = content.as_map().unwrap();
        let order = TotalOrderStamp {
            lamport: op.lamport(),
            client_id: op.client_id(),
        };
        if let Some(slot) = self.state.get_mut(&v.key) {
            if slot.order < order {
                slot.value = v.value;
                slot.order = order;
            }
        } else {
            self.state.insert(
                v.key.to_owned(),
                ValueSlot {
                    value: v.value,
                    order,
                },
            );
        }
    }

    fn track_retreat(&mut self, _: &IdSpanVector) {}

    fn track_forward(&mut self, _: &IdSpanVector) {}

    fn apply_tracked_effects_from(&mut self, _: &crate::VersionVector, _: &IdSpanVector) {}

    fn track_apply(&mut self, op: &RichOp) {
        self.update_state_directly(op);
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
        self.with_container(|map| map.get(&key.into()))
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
