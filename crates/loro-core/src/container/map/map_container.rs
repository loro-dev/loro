use fxhash::FxHashMap;

use crate::{
    container::{Container, ContainerID, ContainerType},
    id::Counter,
    log_store::LogStoreWeakRef,
    op::{InsertContent, Op, RichOp},
    op::{OpContent, RemoteOp},
    span::IdSpan,
    value::{InsertValue, LoroValue},
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
    store: LogStoreWeakRef,
}

#[derive(Debug)]
struct ValueSlot {
    value: InsertValue,
    order: TotalOrderStamp,
    counter: Counter,
}

impl MapContainer {
    #[inline]
    pub(crate) fn new(id: ContainerID, store: LogStoreWeakRef) -> Self {
        MapContainer {
            id,
            store,
            state: FxHashMap::default(),
        }
    }

    pub fn insert(&mut self, key: InternalString, value: InsertValue) {
        let self_id = &self.id;
        let m = self.store.upgrade().unwrap();
        let mut store = m.write().unwrap();
        let client_id = store.this_client_id;
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        let id = store.next_id_for(client_id);
        let counter = id.counter;
        let container = store.get_container_idx(self_id).unwrap();
        store.append_local_ops(&[Op {
            counter: id.counter,
            container,
            content: OpContent::Normal {
                content: InsertContent::Dyn(Box::new(MapSet {
                    key: key.clone(),
                    value: value.clone(),
                })),
            },
        }]);

        self.state.insert(
            key,
            ValueSlot {
                value,
                order,
                counter,
            },
        );
    }

    #[inline]
    pub fn delete(&mut self, key: InternalString) {
        self.insert(key, InsertValue::Null);
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
        for RichOp { op, lamport, .. } in log.iter_ops_at_id_span(id_span, self.id.clone()) {
            match &op.content {
                OpContent::Normal { content } => {
                    let v: &MapSet = content.as_map().unwrap();
                    let order = TotalOrderStamp {
                        lamport,
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
                                counter: op.counter,
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
            map.insert(key.clone(), value.value.clone().into());
        }
        LoroValue::Map(Box::new(map))
    }

    fn checkout_version(&mut self, _vv: &crate::version::VersionVector) {
        todo!()
    }

    fn to_export(&self, _op: &mut Op) {}

    fn to_import(&mut self, _op: &mut RemoteOp) {}
}
