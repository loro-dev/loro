use std::sync::{Arc, Mutex};

use fxhash::FxHashMap;

use crate::{
    container::{
        registry::{ContainerInstance, ContainerWrapper},
        Container, ContainerID, ContainerType,
    },
    context::Context,
    id::ClientID,
    op::RemoteOp,
    op::{Content, Op, RichOp},
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
}

#[derive(Debug)]
struct ValueSlot {
    value: LoroValue,
    order: TotalOrderStamp,
}

// FIXME: make map container support checkout to certain version
impl MapContainer {
    #[inline]
    pub(crate) fn new(id: ContainerID) -> Self {
        MapContainer {
            id,
            state: FxHashMap::default(),
        }
    }

    pub fn insert<C: Context, V: Into<LoroValue>>(
        &mut self,
        ctx: &C,
        key: InternalString,
        value: V,
    ) {
        let value = value.into();
        let self_id = &self.id;
        let m = ctx.log_store();
        let mut store = m.write().unwrap();
        let client_id = store.this_client_id;
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        let id = store.next_id_for(client_id);
        // TODO: store this value?
        let container = store.get_container_idx(self_id).unwrap();
        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: Content::Map(MapSet {
                key: key.clone(),
                value: value.clone(),
            }),
        }]);

        self.state.insert(key, ValueSlot { value, order });
    }

    pub fn insert_obj<C: Context>(
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
            content: Content::Map(MapSet {
                key: key.clone(),
                value: container_id.clone().into(),
            }),
        }]);
        self.state.insert(
            key,
            ValueSlot {
                value: LoroValue::Unresolved(Box::new(container_id.clone())),
                order,
            },
        );
        container_id
    }

    #[inline]
    pub fn delete<C: Context>(&mut self, ctx: &C, key: InternalString) {
        self.insert(ctx, key, LoroValue::Null);
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
            if let Some(container_id) = value.value.as_unresolved() {
                map.insert(
                    key.to_string(),
                    // TODO: make a from
                    LoroValue::Unresolved(container_id.clone()),
                );
            } else {
                map.insert(key.to_string(), value.value.clone());
            }
        }

        map.into()
    }

    fn tracker_checkout(&mut self, _vv: &crate::version::VersionVector) {}

    fn to_export(&mut self, _op: &mut RemoteOp, _gc: bool) {}

    fn to_import(&mut self, _op: &mut RemoteOp) {}

    fn update_state_directly(&mut self, op: &RichOp) {
        let content = op.get_sliced().content;
        let v: &MapSet = content.as_map().unwrap();
        let order = TotalOrderStamp {
            lamport: op.lamport(),
            client_id: op.client_id(),
        };
        if let Some(slot) = self.state.get_mut(&v.key) {
            if slot.order < order {
                slot.value = v.value.clone();
                slot.order = order;
            }
        } else {
            self.state.insert(
                v.key.to_owned(),
                ValueSlot {
                    value: v.value.clone(),
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

    pub fn insert<C: Context, V: Into<LoroValue>>(&mut self, ctx: &C, key: &str, value: V) {
        self.with_container_checked(ctx, |map| {
            map.insert(ctx, key.into(), value);
        })
    }

    pub fn insert_obj<C: Context>(
        &mut self,
        ctx: &C,
        key: &str,
        obj: ContainerType,
    ) -> ContainerID {
        self.with_container_checked(ctx, |map| map.insert_obj(ctx, key.into(), obj))
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, key: &str) {
        self.with_container_checked(ctx, |map| {
            map.delete(ctx, key.into());
        })
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
