use std::{mem, sync::Arc};

use debug_log::debug_dbg;
use fxhash::FxHashMap;
use loro_common::ContainerID;

use crate::{
    container::registry::ContainerIdx,
    delta::MapValue,
    event::{Diff, Index},
    op::{RawOp, RawOpContent},
    refactor::arena::SharedArena,
    InternalString, LoroValue,
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct MapState {
    idx: ContainerIdx,
    map: FxHashMap<InternalString, MapValue>,
    in_txn: bool,
    map_when_txn_start: FxHashMap<InternalString, Option<MapValue>>,
}

impl ContainerState for MapState {
    fn apply_diff(&mut self, diff: &Diff, arena: &SharedArena) {
        if let Diff::NewMap(delta) = diff {
            for (key, value) in delta.updated.iter() {
                if let Some(LoroValue::Container(c)) = &value.value {
                    let idx = arena.register_container(c);
                    arena.set_parent(idx, Some(self.idx));
                }

                let old = self.map.insert(key.clone(), value.clone());
                self.store_txn_snapshot(key.clone(), old);
            }
        }
    }

    fn apply_op(&mut self, op: RawOp, arena: &SharedArena) {
        match op.content {
            RawOpContent::Map(map) => {
                if map.value.is_container() {
                    let idx = arena.register_container(&map.value.as_container().unwrap());
                    arena.set_parent(idx, Some(self.idx));
                }

                self.insert(
                    map.key,
                    MapValue {
                        lamport: (op.lamport, op.id.peer),
                        counter: op.id.counter,
                        value: Some(map.value),
                    },
                )
            }
            RawOpContent::List(_) => unreachable!(),
        }
    }

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        for (key, value) in mem::take(&mut self.map_when_txn_start) {
            if let Some(value) = value {
                self.map.insert(key, value);
            } else {
                self.map.remove(&key);
            }
        }

        self.in_txn = false;
    }

    fn commit_txn(&mut self) {
        self.map_when_txn_start.clear();
        self.in_txn = false;
    }

    fn get_value(&self) -> LoroValue {
        let ans = self.to_map();
        LoroValue::Map(Arc::new(ans))
    }

    #[doc = " Convert a state to a diff that when apply this diff on a empty state,"]
    #[doc = " the state will be the same as this state."]
    fn to_diff(&self) -> Diff {
        Diff::NewMap(crate::delta::MapDelta {
            updated: self.map.clone(),
        })
    }

    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        for (key, value) in self.map.iter() {
            if let Some(LoroValue::Container(x)) = &value.value {
                if x == id {
                    return Some(Index::Key(key.clone()));
                }
            }
        }

        None
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        let mut ans = Vec::new();
        for (_, value) in self.map.iter() {
            if let Some(LoroValue::Container(x)) = &value.value {
                ans.push(x.clone());
            }
        }
        ans
    }
}

impl MapState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            map: FxHashMap::default(),
            in_txn: false,
            map_when_txn_start: FxHashMap::default(),
        }
    }

    fn store_txn_snapshot(&mut self, key: InternalString, old: Option<MapValue>) {
        if self.in_txn && !self.map_when_txn_start.contains_key(&key) {
            self.map_when_txn_start.insert(key, old);
        }
    }

    pub fn insert(&mut self, key: InternalString, value: MapValue) {
        let old = self.map.insert(key.clone(), value);
        if self.in_txn {
            self.store_txn_snapshot(key, old);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&InternalString, &MapValue)> {
        self.map.iter()
    }

    fn len(&self) -> usize {
        self.map.len()
    }

    fn to_map(
        &self,
    ) -> std::collections::HashMap<String, LoroValue, std::hash::BuildHasherDefault<fxhash::FxHasher>>
    {
        let mut ans = FxHashMap::with_capacity_and_hasher(self.len(), Default::default());
        for (key, value) in self.map.iter() {
            if value.value.is_none() {
                continue;
            }

            ans.insert(key.to_string(), value.value.as_ref().cloned().unwrap());
        }
        ans
    }
}
