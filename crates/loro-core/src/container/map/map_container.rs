use std::sync::{Arc, Mutex};

use fxhash::FxHashMap;

use crate::{
    change::Lamport,
    container::{
        registry::{ContainerInstance, ContainerWrapper},
        Container, ContainerID, ContainerType,
    },
    context::Context,
    op::{Content, Op, RichOp},
    op::{OpContent, RemoteOp},
    span::IdSpan,
    value::LoroValue,
    version::TotalOrderStamp,
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
}

#[derive(Debug)]
struct ValueSlot {
    value: LoroValue,
    order: TotalOrderStamp,
}

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
        let counter = id.counter;
        // TODO: store this value?
        let container = store.get_container_idx(self_id).unwrap();
        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: OpContent::Normal {
                content: Content::Map(MapSet {
                    key: key.clone(),
                    value: value.clone(),
                }),
            },
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
        let container_id = store.create_container(obj, self_id.clone());
        // TODO: store this value?
        let id = store.next_id_for(client_id);
        let counter = id.counter;
        let container = store.get_container_idx(self_id).unwrap();
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: OpContent::Normal {
                content: Content::Map(MapSet {
                    key: key.clone(),
                    value: container_id.clone().into(),
                }),
            },
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

    fn apply(&mut self, id_span: IdSpan, log: &LogStore) {
        for RichOp {
            op, lamport, start, ..
        } in log.iter_ops_at_id_span(id_span, self.id.clone())
        {
            match &op.content {
                OpContent::Normal { content } => {
                    if content.as_container().is_some() {
                        continue;
                    }

                    let v: &MapSet = content.as_map().unwrap();
                    let order = TotalOrderStamp {
                        lamport: lamport + start as Lamport,
                        client_id: id_span.client_id,
                    };
                    if let Some(slot) = self.state.get_mut(&v.key) {
                        if slot.order < order {
                            // TODO: can avoid this clone
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
                _ => unreachable!(),
            }
        }
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

    fn checkout_version(&mut self, _vv: &crate::version::VersionVector) {
        todo!()
    }

    fn to_export(&self, _op: &mut RemoteOp) {}

    fn to_import(&mut self, _op: &mut RemoteOp) {}
}

pub struct Map {
    instance: Arc<Mutex<ContainerInstance>>,
}

impl Clone for Map {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
        }
    }
}

impl Map {
    pub fn insert<C: Context, V: Into<LoroValue>>(&mut self, ctx: &C, key: &str, value: V) {
        self.with_container(|map| {
            map.insert(ctx, key.into(), value);
        })
    }

    pub fn insert_obj<C: Context>(
        &mut self,
        ctx: &C,
        key: &str,
        obj: ContainerType,
    ) -> ContainerID {
        self.with_container(|map| map.insert_obj(ctx, key.into(), obj))
    }

    pub fn delete<C: Context>(&mut self, ctx: &C, key: &str) {
        self.with_container(|map| {
            map.delete(ctx, key.into());
        })
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
}

impl From<Arc<Mutex<ContainerInstance>>> for Map {
    fn from(map: Arc<Mutex<ContainerInstance>>) -> Self {
        Map { instance: map }
    }
}
