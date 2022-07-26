use std::{pin::Pin, ptr::NonNull, rc::Weak};

use fxhash::FxHashMap;
use serde::Serialize;

use crate::{
    container::{Container, ContainerID, ContainerType},
    id::{Counter, ID},
    op::{utils::downcast_ref, Op},
    op::{OpContent, OpProxy},
    value::{InsertValue, LoroValue},
    version::TotalOrderStamp,
    ClientID, InternalString, Lamport, LogStore, OpType,
};

use super::MapInsertContent;

/// We can only insert to Map
/// delete = set null
///
#[derive(Debug)]
pub struct MapContainer {
    id: ContainerID,
    state: FxHashMap<InternalString, ValueSlot>,
    log_store: NonNull<LogStore>,
    value: Option<LoroValue>,
}

#[derive(Debug)]
struct ValueSlot {
    value: InsertValue,
    order: TotalOrderStamp,
    counter: Counter,
}

impl MapContainer {
    #[inline]
    pub fn new(id: ContainerID, store: NonNull<LogStore>) -> Self {
        MapContainer {
            id,
            state: FxHashMap::default(),
            log_store: store,
            value: None,
        }
    }

    fn log_store(&mut self) -> &mut LogStore {
        unsafe { self.log_store.as_mut() }
    }

    pub fn insert(&mut self, key: InternalString, value: InsertValue) {
        let self_id = self.id.clone();
        let store = self.log_store();
        let client_id = store.this_client_id;
        let order = TotalOrderStamp {
            client_id,
            lamport: store.next_lamport(),
        };

        let id = store.next_id(client_id);
        let counter = id.counter;
        store.append_local_ops(vec![Op {
            id,
            container: self_id,
            content: OpContent::Normal {
                content: Box::new(MapInsertContent {
                    key: key.clone(),
                    value: value.clone(),
                }),
            },
        }]);

        if self.value.is_some() {
            self.value
                .as_mut()
                .unwrap()
                .as_map_mut()
                .unwrap()
                .insert(key.clone(), value.clone().into());
        }

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

    fn apply(&mut self, op: &OpProxy) {
        debug_assert_eq!(&op.op().container, self.id());
        match op.content() {
            OpContent::Normal { content } => {
                let v: &MapInsertContent = downcast_ref(&**content).unwrap();
                let order = TotalOrderStamp {
                    lamport: op.lamport(),
                    client_id: op.id().client_id,
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
                            counter: op.id().counter,
                        },
                    );

                    if self.value.is_some() {
                        self.value
                            .as_mut()
                            .unwrap()
                            .as_map_mut()
                            .unwrap()
                            .insert(v.key.clone(), v.value.clone().into());
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn get_value(&mut self) -> &LoroValue {
        if self.value.is_none() {
            let mut map = FxHashMap::default();
            for (key, value) in self.state.iter() {
                map.insert(key.clone(), value.value.clone().into());
            }
            self.value = Some(LoroValue::Map(map));
        }

        self.value.as_ref().unwrap()
    }

    fn checkout_version(&mut self, vv: &crate::version::VersionVector) {
        todo!()
    }
}
