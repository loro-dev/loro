use std::{mem, sync::Arc};

use fxhash::FxHashMap;
use loro_common::{ContainerID, LoroResult};

use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, map::MapSet},
    delta::MapValue,
    event::{Index, InternalDiff, UnresolvedDiff},
    op::{Op, RawOp, RawOpContent},
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
    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
    ) -> UnresolvedDiff {
        let InternalDiff::Map(delta) = diff else {
            unreachable!()
        };

        for (key, value) in delta.updated.iter() {
            if let Some(LoroValue::Container(c)) = &value.value {
                let idx = arena.register_container(c);
                arena.set_parent(idx, Some(self.idx));
            }

            let old = self.map.insert(key.clone(), value.clone());
            self.store_txn_snapshot(key.clone(), old);
        }

        UnresolvedDiff::NewMap(delta)
    }

    fn apply_op(&mut self, op: &RawOp, _: &Op, arena: &SharedArena) -> LoroResult<()> {
        match &op.content {
            RawOpContent::Map(MapSet { key, value }) => {
                if value.is_none() {
                    self.insert(
                        key.clone(),
                        MapValue {
                            lamport: (op.lamport, op.id.peer),
                            counter: op.id.counter,
                            value: None,
                        },
                    );
                    return Ok(());
                }
                let value = value.clone().unwrap();
                if value.is_container() {
                    let idx = arena.register_container(value.as_container().unwrap());
                    arena.set_parent(idx, Some(self.idx));
                }

                self.insert(
                    key.clone(),
                    MapValue {
                        lamport: (op.lamport, op.id.peer),
                        counter: op.id.counter,
                        value: Some(value),
                    },
                );
                Ok(())
            }
            RawOpContent::List(_) => unreachable!(),
            RawOpContent::Tree(_) => unreachable!(),
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

    fn get_value(&mut self) -> LoroValue {
        let ans = self.to_map();
        LoroValue::Map(Arc::new(ans))
    }

    #[doc = " Convert a state to a diff that when apply this diff on a empty state,"]
    #[doc = " the state will be the same as this state."]
    fn to_diff(&mut self) -> UnresolvedDiff {
        UnresolvedDiff::NewMap(crate::delta::MapDelta {
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

    pub fn iter(
        &self,
    ) -> std::collections::hash_map::Iter<
        '_,
        string_cache::Atom<string_cache::EmptyStaticAtomSet>,
        MapValue,
    > {
        self.map.iter()
    }

    pub fn len(&self) -> usize {
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

    pub fn get(&self, k: &str) -> Option<&LoroValue> {
        match self.map.get(&k.into()) {
            Some(value) => match &value.value {
                Some(v) => Some(v),
                None => None,
            },
            None => None,
        }
    }
}
