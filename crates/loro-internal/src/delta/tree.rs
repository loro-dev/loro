use fxhash::FxHashMap;
use serde::Serialize;

use crate::state::TreeID;

#[derive(Debug, Clone, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: FxHashMap<TreeID, Option<Option<TreeID>>>,
}

impl TreeDelta {
    pub(crate) fn compose(&self, x: TreeDelta) -> TreeDelta {
        // TODO: lamport sort?
        let mut diff = self.diff.clone();
        for (k, v) in x.diff.into_iter() {
            if let Some(o) = diff.get_mut(&k) {
                *o = v;
            } else {
                diff.insert(k, v);
            }
        }
        TreeDelta { diff }
    }
}
