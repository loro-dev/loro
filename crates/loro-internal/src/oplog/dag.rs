use std::cmp::Ordering;
use std::fmt::Display;

use crate::change::Lamport;
use crate::dag::{Dag, DagNode};
use crate::id::{Counter, ID};
use crate::span::{HasId, HasLamport};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use loro_common::{HasCounter, HasCounterSpan, HasIdSpan};
use once_cell::sync::OnceCell;
use rle::{HasIndex, HasLength, Mergable, Sliceable};

use super::{AppDag, AppDagNode};

impl HasIndex for AppDagNode {
    type Int = Counter;
    fn get_start_index(&self) -> Self::Int {
        self.cnt
    }

    fn get_end_index(&self) -> Self::Int {
        self.cnt + self.len as Counter
    }
}

impl Sliceable for AppDagNode {
    fn slice(&self, from: usize, to: usize) -> Self {
        AppDagNode {
            peer: self.peer,
            cnt: self.cnt + from as Counter,
            lamport: self.lamport + from as Lamport,
            deps: if from > 0 {
                Frontiers::from_id(self.id_start().inc(from as Counter - 1))
            } else {
                self.deps.clone()
            },
            vv: if let Some(vv) = self.vv.get() {
                let mut new = vv.clone();
                new.insert(self.peer, self.cnt + from as Counter);
                OnceCell::with_value(new)
            } else {
                OnceCell::new()
            },
            has_succ: if to == self.len { self.has_succ } else { true },
            len: to - from,
        }
    }
}

impl HasId for AppDagNode {
    fn id_start(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.cnt,
        }
    }
}

impl HasCounter for AppDagNode {
    fn ctr_start(&self) -> Counter {
        self.cnt
    }
}

impl HasLength for AppDagNode {
    fn atom_len(&self) -> usize {
        self.len
    }

    fn content_len(&self) -> usize {
        self.len
    }
}

impl Mergable for AppDagNode {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}

impl HasLamport for AppDagNode {
    fn lamport(&self) -> Lamport {
        self.lamport
    }
}

impl DagNode for AppDagNode {
    fn deps(&self) -> &[ID] {
        &self.deps
    }
}

impl Dag for AppDag {
    type Node = AppDagNode;

    fn frontier(&self) -> &[ID] {
        &self.frontiers
    }

    fn get(&self, id: ID) -> Option<&Self::Node> {
        let x = self.map.range(..=id).next_back()?;
        if x.1.contains_id(id) {
            Some(x.1)
        } else {
            None
        }
    }

    fn vv(&self) -> VersionVector {
        self.vv.clone()
    }
}

impl AppDag {
    // PERF: this may be painfully slow
    /// get the version vector for a certain op.
    /// It's the version when the op is applied
    pub fn get_vv(&self, id: ID) -> Option<ImVersionVector> {
        self.get(id).map(|x| {
            let mut vv = self.ensure_vv_for(x);
            vv.insert(id.peer, id.counter + 1);
            vv
        })
    }

    fn ensure_vv_for(&self, node: &AppDagNode) -> ImVersionVector {
        if let Some(vv) = node.vv.get() {
            return vv.clone();
        }

        let mut ans_vv = ImVersionVector::default();
        for id in node.deps.iter() {
            let node = self.get(*id).expect("deps should be in the dag");
            let dep_vv = self.ensure_vv_for(node);
            if ans_vv.is_empty() {
                ans_vv = dep_vv;
            } else {
                ans_vv.extend_to_include_vv(dep_vv.iter());
            }

            ans_vv.insert(node.peer, node.ctr_last());
        }

        node.vv.set(ans_vv.clone()).unwrap();
        ans_vv
    }

    /// Compare the causal order of two versions.
    /// If None, two versions are concurrent to each other
    pub fn cmp_version(&self, a: ID, b: ID) -> Option<Ordering> {
        if a.peer == b.peer {
            return Some(a.counter.cmp(&b.counter));
        }

        let a = self.get_vv(a).unwrap();
        let b = self.get_vv(b).unwrap();
        a.partial_cmp(&b)
    }

    pub fn get_lamport(&self, id: &ID) -> Option<Lamport> {
        self.get(*id).and_then(|node| {
            assert!(id.counter >= node.cnt);
            if node.cnt + node.len as Counter > id.counter {
                Some(node.lamport + (id.counter - node.cnt) as Lamport)
            } else {
                None
            }
        })
    }

    pub fn get_change_lamport_from_deps(&self, deps: &[ID]) -> Option<Lamport> {
        let mut lamport = 0;
        for id in deps.iter() {
            let x = self.get_lamport(id)?;
            lamport = lamport.max(x + 1);
        }

        Some(lamport)
    }

    /// Convert a frontiers to a version vector
    ///
    /// If the frontiers version is not found in the dag, return None
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        let mut vv: VersionVector = Default::default();
        for id in frontiers.iter() {
            let x = self.get(*id)?;
            let target_vv = self.ensure_vv_for(x);
            vv.extend_to_include_vv(target_vv.iter());
            vv.extend_to_include_last_id(*id);
        }

        Some(vv)
    }

    pub(crate) fn frontiers_to_im_vv(&self, frontiers: &Frontiers) -> ImVersionVector {
        if frontiers.is_empty() {
            return Default::default();
        }

        let mut vv = {
            let id = frontiers[0];
            let Some(x) = self.get(id) else {
                unreachable!()
            };
            let mut vv = self.ensure_vv_for(x);
            vv.extend_to_include_last_id(id);
            vv
        };

        for id in frontiers[1..].iter() {
            let Some(x) = self.get(*id) else {
                unreachable!()
            };
            let x = self.ensure_vv_for(x);
            vv.extend_to_include_vv(x.iter());
            vv.extend_to_include_last_id(*id);
        }

        vv
    }

    #[inline(always)]
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        vv.to_frontiers(self)
    }

    pub(crate) fn frontiers_to_next_lamport(&self, frontiers: &Frontiers) -> Lamport {
        if frontiers.is_empty() {
            return 0;
        }

        let mut lamport = {
            let id = frontiers[0];
            let Some(x) = self.get(id) else {
                unreachable!()
            };
            (id.counter - x.cnt) as Lamport + x.lamport + 1
        };

        for id in frontiers[1..].iter() {
            let Some(x) = self.get(*id) else {
                unreachable!()
            };
            lamport = lamport.max((id.counter - x.cnt) as Lamport + x.lamport + 1);
        }

        lamport
    }

    pub fn get_frontiers(&self) -> &Frontiers {
        &self.frontiers
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        if &self.frontiers == other {
            Ordering::Equal
        } else if other.iter().all(|id| self.vv.includes_id(*id)) {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }

    // PERF
    /// Compare two [Frontiers] causally.
    ///
    /// If one of the [Frontiers] are not included, it will return [FrontiersNotIncluded].
    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        let a = self.frontiers_to_vv(a).ok_or(FrontiersNotIncluded)?;
        let b = self.frontiers_to_vv(b).ok_or(FrontiersNotIncluded)?;
        Ok(a.partial_cmp(&b))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FrontiersNotIncluded;
impl Display for FrontiersNotIncluded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The given Frontiers are not included by the doc")
    }
}
