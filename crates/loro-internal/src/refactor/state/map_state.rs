use std::mem;

use fxhash::FxHashMap;

use crate::{
    delta::MapValue,
    event::Diff,
    op::{RawOp, RawOpContent},
    InternalString,
};

use super::ContainerState;

#[derive(Clone)]
pub struct MapState {
    map: FxHashMap<InternalString, MapValue>,
    in_txn: bool,
    map_when_txn_start: FxHashMap<InternalString, Option<MapValue>>,
}

impl ContainerState for MapState {
    fn apply_diff(&mut self, diff: Diff) {
        if let Diff::NewMap(delta) = diff {
            for (key, value) in delta.updated {
                let old = self.map.insert(key.clone(), value);
                self.store_txn_snapshot(key, old);
            }
        }
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

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn commit_txn(&mut self) {
        self.map_when_txn_start.clear();
        self.in_txn = false;
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
}
