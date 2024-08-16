use crate::change::{Change, Lamport};
use crate::dag::{Dag, DagNode};
use crate::id::{Counter, ID};
use crate::span::{HasId, HasLamport};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use loro_common::{HasCounter, HasCounterSpan, HasIdSpan, PeerID};
use once_cell::sync::OnceCell;
use rle::{HasIndex, HasLength, Mergable, Sliceable};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Mutex;

use super::ChangeStore;

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug)]
pub struct AppDag {
    change_store: ChangeStore,
    /// It only contains nodes that are already parsed.
    ///
    /// - All the unparsed op ids must be included in `unparsed_vv`.
    /// - All the parsed and unparsed op ids must be included in `vv`.
    map: Mutex<BTreeMap<ID, AppDagNode>>,
    /// The latest known frontiers
    frontiers: Frontiers,
    /// The latest known version vector
    vv: VersionVector,
    /// Ops included in the version vector but not parsed yet
    ///
    /// # Invariants
    ///
    /// - `vv` >= `unparsed_vv`
    unparsed_vv: VersionVector,
    /// It's a set of points which are deps of some parsed ops.
    /// But the ops in this set are not parsed yet. When they are parsed,
    /// we need to make sure it breaks at the given point.
    unhandled_dep_points: BTreeSet<ID>,
}

pub(crate) struct EnsureDagNodeDepsAreAtTheEnd {
    _private: PhantomData<()>,
}

#[derive(Debug, Clone)]
pub struct AppDagNode {
    pub(crate) peer: PeerID,
    pub(crate) cnt: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) deps: Frontiers,
    pub(crate) vv: OnceCell<ImVersionVector>,
    /// A flag indicating whether any other nodes depend on this node.
    /// The calculation of frontiers is based on this value.
    pub(crate) has_succ: bool,
    pub(crate) len: usize,
}

impl AppDag {
    pub(super) fn new(change_store: ChangeStore) -> Self {
        Self {
            change_store,
            map: Mutex::new(BTreeMap::new()),
            frontiers: Frontiers::default(),
            vv: VersionVector::default(),
            unparsed_vv: VersionVector::default(),
            unhandled_dep_points: BTreeSet::new(),
        }
    }

    pub fn frontiers(&self) -> &Frontiers {
        &self.frontiers
    }

    pub fn vv(&self) -> &VersionVector {
        &self.vv
    }

    pub fn is_empty(&self) -> bool {
        self.vv.is_empty()
    }

    pub(super) fn handle_new_change(&mut self, change: &Change) -> EnsureDagNodeDepsAreAtTheEnd {
        let len = change.content_len();
        self.update_frontiers(change.id_last(), &change.deps);
        if change.deps_on_self() {
            // don't need to push new element to dag because it only depends on itself
            self.with_last_mut_of_peer(change.id.peer, |last| {
                let last = last.unwrap();
                assert_eq!(last.peer, change.id.peer, "peer id is not the same");
                assert_eq!(
                    last.cnt + last.len as Counter,
                    change.id.counter,
                    "counter is not continuous"
                );
                assert_eq!(
                    last.lamport + last.len as Lamport,
                    change.lamport,
                    "lamport is not continuous"
                );
                last.len = (change.id.counter - last.cnt) as usize + len;
                last.has_succ = false;
            });
        } else {
            let vv = self.frontiers_to_im_vv(&change.deps);
            let mut pushed = false;
            let node = AppDagNode {
                vv: OnceCell::from(vv),
                peer: change.id.peer,
                cnt: change.id.counter,
                lamport: change.lamport,
                deps: change.deps.clone(),
                has_succ: false,
                len,
            };

            self.with_last_mut_of_peer(change.id.peer, |last| {
                if let Some(last) = last {
                    if change.id.counter > 0 {
                        assert_eq!(
                            last.ctr_end(),
                            change.id.counter,
                            "counter is not continuous"
                        );
                    }

                    if last.is_mergable(&node, &()) {
                        pushed = true;
                        last.merge(&node, &());
                    }
                }
            });

            if !pushed {
                self.insert(node.id_start(), node);
            }

            for dep in change.deps.iter() {
                let ans = self.with_node_mut(*dep, |target| {
                    let target = target.unwrap();
                    if target.ctr_last() == dep.counter {
                        target.has_succ = true;
                        None
                    } else {
                        // We need to split the target node into two part
                        // so that we can ensure the new change depends on the
                        // last id of a dag node.
                        let new_node =
                            target.slice(dep.counter as usize - target.cnt as usize, target.len);
                        target.len -= new_node.len;
                        Some(new_node)
                    }
                });
                if let Some(new_node) = ans {
                    self.insert(new_node.id_start(), new_node);
                }
            }
        }

        EnsureDagNodeDepsAreAtTheEnd {
            _private: PhantomData,
        }
    }

    fn insert(&self, id: ID, node: AppDagNode) {
        self.map.lock().unwrap().insert(id, node);
    }

    pub(crate) fn with_node_mut<R>(
        &self,
        id: ID,
        f: impl FnOnce(Option<&mut AppDagNode>) -> R,
    ) -> R {
        self.ensure_lazy_load_node(id);
        let mut map = self.map.lock().unwrap();
        let x = map.range_mut(..=id).next_back();
        if let Some((_, node)) = x {
            if node.contains_id(id) {
                f(Some(node))
            } else {
                f(None)
            }
        } else {
            f(None)
        }
    }

    /// If the lamport of change can be calculated, return Ok, otherwise, Err
    pub(crate) fn calc_unknown_lamport_change(&self, change: &mut Change) -> Result<(), ()> {
        for dep in change.deps.iter() {
            match self.get_lamport(dep) {
                Some(lamport) => {
                    change.lamport = change.lamport.max(lamport + 1);
                }
                None => return Err(()),
            }
        }
        Ok(())
    }

    pub(crate) fn find_deps_of_id(&self, id: ID) -> Frontiers {
        self.ensure_lazy_load_node(id);
        let Some(node) = self.get(id) else {
            return Frontiers::default();
        };

        let offset = id.counter - node.cnt;
        if offset == 0 {
            node.deps.clone()
        } else {
            ID::new(id.peer, node.cnt + offset - 1).into()
        }
    }

    pub(crate) fn with_last_mut_of_peer<R>(
        &mut self,
        peer: PeerID,
        f: impl FnOnce(Option<&mut AppDagNode>) -> R,
    ) -> R {
        self.lazy_load_last_of_peer(peer);
        let mut binding = self.map.lock().unwrap();
        let last = binding
            .range_mut(..=ID::new(peer, Counter::MAX))
            .next_back()
            .map(|(_, v)| v);
        f(last)
    }

    pub(super) fn update_frontiers(&mut self, id: ID, deps: &Frontiers) {
        self.frontiers.update_frontiers_on_new_change(id, deps);
        self.vv.extend_to_include_last_id(id);
    }

    pub(super) fn lazy_load_last_of_peer(&mut self, peer: u64) {
        if !self.unparsed_vv.contains_key(&peer) {
            return;
        }

        todo!()
    }

    pub(super) fn ensure_lazy_load_node(&self, id: ID) {
        if !self.unparsed_vv.includes_id(id) {
            return;
        }

        todo!("load dag node from kv store, from id -> unparsed_vv.get(peer)")
    }

    pub(super) fn fork(&self, change_store: ChangeStore) -> AppDag {
        AppDag {
            change_store: change_store.clone(),
            map: Mutex::new(self.map.lock().unwrap().clone()),
            frontiers: self.frontiers.clone(),
            vv: self.vv.clone(),
            unparsed_vv: self.unparsed_vv.clone(),
            unhandled_dep_points: self.unhandled_dep_points.clone(),
        }
    }

    pub fn total_parsed_dag_node(&self) -> usize {
        self.map.lock().unwrap().len()
    }
}

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

    fn get(&self, id: ID) -> Option<Self::Node> {
        self.ensure_lazy_load_node(id);
        let binding = self.map.lock().unwrap();
        let x = binding.range(..=id).next_back()?;
        if x.1.contains_id(id) {
            // PERF: do we need to optimize clone like this?
            // by adding another layer of Arc?
            Some(x.1.clone())
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
            let mut vv = self.ensure_vv_for(&x);
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
            let dep_vv = self.ensure_vv_for(&node);
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
        self.ensure_lazy_load_node(*id);
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
            let target_vv = self.ensure_vv_for(&x);
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
            let mut vv = self.ensure_vv_for(&x);
            vv.extend_to_include_last_id(id);
            vv
        };

        for id in frontiers[1..].iter() {
            let Some(x) = self.get(*id) else {
                unreachable!()
            };
            let x = self.ensure_vv_for(&x);
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
