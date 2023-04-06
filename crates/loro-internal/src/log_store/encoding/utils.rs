use fxhash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{id::ClientID, VersionVector};

use super::{encode_snapshot::Snapshot, EncodeBuffer};

#[derive(Debug, Default)]
pub(super) struct BatchSnapshotSelector<'a> {
    snapshots: SmallVec<[Snapshot<'a>; 1]>,
    candidates: FxHashMap<ClientID, SmallVec<[usize; 1]>>,
    max_end_vv: VersionVector,
}

impl<'a> BatchSnapshotSelector<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_snapshot(&mut self, mut snapshot: Snapshot<'a>) {
        let vv = snapshot.calc_end_vv();
        let idx = self.snapshots.len();
        let mut candidate = false;
        for (&k, &v) in vv.iter() {
            if let Some(old) = self.max_end_vv.get(&k) {
                match v.cmp(old) {
                    std::cmp::Ordering::Greater => {
                        self.max_end_vv.insert(k, v);
                        candidate = true;
                        self.candidates.insert(k, SmallVec::from_slice(&[idx]));
                    }
                    std::cmp::Ordering::Equal => {
                        if let Some(candidates) = self.candidates.get_mut(&k) {
                            candidates.push(idx);
                            candidate = true;
                        }
                    }
                    std::cmp::Ordering::Less => {}
                }
            } else {
                self.max_end_vv.insert(k, v);
                candidate = true;
                self.candidates.insert(k, SmallVec::from_slice(&[idx]));
            }
        }
        if candidate {
            self.snapshots.push(snapshot);
        }
    }

    pub fn select(self) -> Vec<Snapshot<'a>> {
        // The snapshot has different version, We need to choose the least number to ensure that the largest version range is covered.
        let mut ans = Vec::new();
        // The simple way is to choose the last of each client.
        let mut selected = FxHashSet::default();
        for (_, candidates) in self.candidates {
            let idx = candidates.len() - 1;
            if !selected.contains(&idx) {
                selected.insert(idx);
            }
        }
        self.snapshots
            .into_iter()
            .enumerate()
            .for_each(|(idx, snapshot)| {
                if selected.contains(&idx) {
                    ans.push(snapshot);
                }
            });
        ans
    }
}
