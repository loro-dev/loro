use std::{pin::Pin, ptr::NonNull, rc::Weak};

use fxhash::FxHashMap;

use crate::{
    container::{Container, ContainerID, ContainerType},
    op::content::downcast_ref,
    op::OpProxy,
    value::{InsertValue, LoroValue},
    ClientID, InternalString, Lamport, LogStore, OpType, Snapshot,
};

use super::MapInsertContent;

/// we can only insert to Map
/// delete = set null
///
#[derive(Debug)]
pub struct MapContainer {
    id: ContainerID,
    state: FxHashMap<InternalString, ValueSlot>,
    snapshot: Option<Snapshot>,
    log_store: NonNull<LogStore>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
struct TotalOrder {
    lamport: Lamport,
    client_id: ClientID,
}

#[derive(Debug)]
struct ValueSlot {
    value: InsertValue,
    order: TotalOrder,
}

impl MapContainer {
    #[inline]
    pub fn new(id: ContainerID, store: NonNull<LogStore>) -> Self {
        MapContainer {
            id,
            state: FxHashMap::default(),
            snapshot: None,
            log_store: store,
        }
    }

    fn log_store(&mut self) -> &mut LogStore {
        unsafe { self.log_store.as_mut() }
    }

    pub fn insert(&mut self, key: InternalString, value: InsertValue) {
        let store = self.log_store();
        let order = TotalOrder {
            lamport: store.next_lamport(),
            client_id: store.this_client_id,
        };
        self.state.insert(key, ValueSlot { value, order });
    }
}

impl Container for MapContainer {
    #[inline(always)]
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn type_id(&self) -> ContainerType {
        ContainerType::Map
    }

    fn apply(&mut self, op: &OpProxy) {
        match op.content() {
            crate::OpContent::Insert { container, content } => {
                debug_assert!(*container == self.id);
                let v: &MapInsertContent = downcast_ref(&**content).unwrap();
                let order = TotalOrder {
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
                        },
                    );
                }
            }
            _ => unreachable!(),
        }
    }

    fn snapshot(&mut self) -> &crate::Snapshot {
        if self.snapshot.is_none() {
            let mut map = FxHashMap::default();
            for (key, value) in self.state.iter() {
                map.insert(key.clone(), value.value.clone().into());
            }

            self.snapshot = Some(Snapshot::new(LoroValue::Map(map)));
        }

        self.snapshot.as_ref().unwrap()
    }

    fn checkout_version(&mut self, _vv: &crate::version::VersionVector, _log: &crate::LogStore) {
        todo!()
    }
}
