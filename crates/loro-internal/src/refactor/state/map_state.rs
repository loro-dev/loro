use std::mem;

use fxhash::FxHashMap;

use crate::{delta::MapValue, event::Diff, InternalString};

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
}

impl MapState {
    fn store_txn_snapshot(&mut self, key: InternalString, old: Option<MapValue>) {
        if self.in_txn && !self.map_when_txn_start.contains_key(&key) {
            self.map_when_txn_start.insert(key, old);
        }
    }
}
