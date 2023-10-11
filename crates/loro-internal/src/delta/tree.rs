use loro_common::TreeID;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<(TreeID, Option<TreeID>)>,
}

// TODO: tree
pub enum TreeDiff {
    Create,
    Move(Option<TreeID>),
    Delete,
}

impl TreeDelta {
    // TODO:
    pub(crate) fn compose(&self, x: TreeDelta) -> TreeDelta {
        todo!();
        // let mut diff = self
        //     .diff
        //     .iter()
        //     .map(|(k, v)| (*k, *v))
        //     .collect::<FxHashMap<_, _>>();
        // for (k, v) in x.diff.into_iter() {
        //     if let Some(old) = diff.get_mut(&k) {
        //         if &v > old {
        //             *old = v;
        //         }
        //     } else {
        //         diff.insert(k, v);
        //     }
        // }
        // let diff = diff
        //     .into_iter()
        //     .sorted_by_key(|(_, v)| *v)
        //     .collect::<Vec<_>>();
        // TreeDelta { diff }
    }
}
