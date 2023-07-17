use std::{mem, sync::Arc};

use fxhash::FxHashMap;

use crate::{
    delta::MapValue,
    event::Diff,
    op::{RawOp, RawOpContent},
    refactor::arena::SharedArena,
    InternalString, LoroValue,
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct MapState {
    map: FxHashMap<InternalString, MapValue>,
    in_txn: bool,
    map_when_txn_start: FxHashMap<InternalString, Option<MapValue>>,
}

impl ContainerState for MapState {
    fn apply_diff(&mut self, diff: &Diff, _arena: &SharedArena) {
        if let Diff::NewMap(delta) = diff {
            for (key, value) in delta.updated.iter() {
                let old = self.map.insert(key.clone(), value.clone());
                self.store_txn_snapshot(key.clone(), old);
            }
        }
    }

    fn apply_op(&mut self, op: RawOp) {
        match op.content {
            RawOpContent::Map(map) => self.insert(
                map.key,
                MapValue {
                    lamport: (op.lamport, op.id.peer),
                    counter: op.id.counter,
                    value: Some(map.value),
                },
            ),
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
        let mut ans = FxHashMap::with_capacity_and_hasher(self.len(), Default::default());
        for (key, value) in self.map.iter() {
            if value.value.is_none() {
                continue;
            }

            ans.insert(key.to_string(), value.value.as_ref().cloned().unwrap());
        }

        LoroValue::Map(Arc::new(ans))
    }
}

impl MapState {
    pub fn new() -> Self {
        Self {
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
}
