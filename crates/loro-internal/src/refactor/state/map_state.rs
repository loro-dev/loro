use fxhash::FxHashMap;

use crate::{delta::MapValue, event::Diff, InternalString};

use super::ContainerState;

#[derive(Clone)]
pub struct MapState {
    map: FxHashMap<InternalString, MapValue>,
}

impl ContainerState for MapState {
    fn apply_diff(&mut self, diff: Diff) {
        if let Diff::NewMap(delta) = diff {
            for (key, value) in delta.updated {
                self.map.insert(key, value);
            }
        }
    }
}
