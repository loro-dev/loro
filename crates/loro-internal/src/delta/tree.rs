use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::TreeID;
use serde::Serialize;

use crate::change::Lamport;

#[derive(Debug, Clone, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<(TreeID, TreeDiff)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum TreeDiff {
    Delete,
    Move((Lamport, Option<TreeID>)),
}

impl TreeDelta {
    pub(crate) fn compose(&self, x: TreeDelta) -> TreeDelta {
        let mut diff = self
            .diff
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect::<FxHashMap<_, _>>();
        for (k, v) in x.diff.into_iter() {
            if let Some(old) = diff.get_mut(&k) {
                if &v > old {
                    *old = v;
                }
            } else {
                diff.insert(k, v);
            }
        }
        let diff = diff
            .into_iter()
            .sorted_by_key(|(_, v)| *v)
            .collect::<Vec<_>>();
        TreeDelta { diff }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_diff_ord() {
        let mut v = vec![
            TreeDiff::Delete,
            TreeDiff::Move((0, None)),
            TreeDiff::Move((1, None)),
        ];
        v.sort();
        assert_eq!(
            v,
            vec![
                TreeDiff::Delete,
                TreeDiff::Move((0, None)),
                TreeDiff::Move((1, None))
            ]
        )
    }
}
