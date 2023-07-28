use std::cmp::Ordering;

use crate::change::Lamport;
use crate::dag::{Dag, DagNode};
use crate::id::{Counter, ID};
use crate::span::{HasId, HasLamport};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
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
            deps: Default::default(),
            vv: Default::default(),
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
        let ID {
            peer: client_id,
            counter,
        } = id;
        self.map
            .get(&client_id)
            .and_then(|rle| rle.get_by_atom_index(counter).map(|x| x.element))
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
        self.map.get(&id.peer).and_then(|rle| {
            rle.get_by_atom_index(id.counter).map(|x| {
                let mut vv = x.element.vv.clone();
                vv.insert(id.peer, id.counter + 1);
                vv
            })
        })
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
        self.map.get(&id.peer).and_then(|rle| {
            rle.get_by_atom_index(id.counter)
                .map(|x| x.element.lamport + (id.counter - x.element.cnt) as Lamport)
        })
    }

    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> VersionVector {
        let mut vv: VersionVector = Default::default();
        for id in frontiers.iter() {
            let Some(rle) = self.map.get(&id.peer) else { continue };
            let Some(x) = rle.get_by_atom_index(id.counter) else { continue };
            vv.extend_to_include_vv(x.element.vv.iter());
            vv.extend_to_include_last_id(*id);
        }

        vv
    }

    pub fn frontiers_to_im_vv(&self, frontiers: &Frontiers) -> ImVersionVector {
        if frontiers.is_empty() {
            return Default::default();
        }

        let mut vv = {
            let id = frontiers[0];
            let Some(rle) = self.map.get(&id.peer) else { unreachable!() };
            let Some(x) = rle.get_by_atom_index(id.counter) else { unreachable!() };
            let mut vv = x.element.vv.clone();
            vv.extend_to_include_last_id(id);
            vv
        };

        for id in frontiers[1..].iter() {
            let Some(rle) = self.map.get(&id.peer) else { unreachable!() };
            let Some(x) = rle.get_by_atom_index(id.counter) else { unreachable!() };
            vv.extend_to_include_vv(x.element.vv.iter());
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
            let Some(rle) = self.map.get(&id.peer) else { unreachable!() };
            let Some(x) = rle.get_by_atom_index(id.counter) else { unreachable!("{} not found", id) };
            (id.counter - x.element.cnt) as Lamport + x.element.lamport + 1
        };

        for id in frontiers[1..].iter() {
            let Some(rle) = self.map.get(&id.peer) else { unreachable!() };
            let Some(x) = rle.get_by_atom_index(id.counter) else { unreachable!() };
            lamport = lamport.max((id.counter - x.element.cnt) as Lamport + x.element.lamport + 1);
        }

        lamport
    }

    pub fn get_frontiers(&self) -> &Frontiers {
        &self.frontiers
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    pub fn cmp_frontiers(&self, other: &Frontiers) -> Ordering {
        if &self.frontiers == other {
            Ordering::Equal
        } else if other.iter().all(|id| self.vv.includes_id(*id)) {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }
}
