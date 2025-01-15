use std::{cmp::Ordering, collections::HashMap, sync::RwLock};

use loro::{CounterSpan, IdSpan, LoroResult, PeerID, ID};

pub struct VersionVector(RwLock<loro::VersionVector>);

impl Default for VersionVector {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionVector {
    pub fn new() -> Self {
        Self(RwLock::new(loro::VersionVector::default()))
    }

    pub fn diff(&self, rhs: &Self) -> VersionVectorDiff {
        self.0.read().unwrap().diff(&rhs.0.read().unwrap()).into()
    }

    pub fn get_last(&self, peer: PeerID) -> Option<i32> {
        self.0.read().unwrap().get_last(peer)
    }

    pub fn set_last(&self, id: ID) {
        self.0.write().unwrap().set_last(id);
    }

    pub fn set_end(&self, id: ID) {
        self.0.write().unwrap().set_end(id);
    }

    pub fn get_missing_span(&self, target: &Self) -> Vec<IdSpan> {
        self.0
            .read()
            .unwrap()
            .get_missing_span(&target.0.read().unwrap())
    }

    pub fn merge(&self, other: &VersionVector) {
        self.0.write().unwrap().merge(&other.0.read().unwrap())
    }

    pub fn includes_vv(&self, other: &VersionVector) -> bool {
        self.0.read().unwrap().includes_vv(&other.0.read().unwrap())
    }

    pub fn includes_id(&self, id: ID) -> bool {
        self.0.read().unwrap().includes_id(id)
    }

    pub fn intersect_span(&self, target: IdSpan) -> Option<CounterSpan> {
        self.0.read().unwrap().intersect_span(target)
    }

    pub fn extend_to_include_vv(&self, other: &VersionVector) {
        self.0
            .write()
            .unwrap()
            .extend_to_include_vv(other.0.read().unwrap().iter());
    }

    pub fn partial_cmp(&self, other: &VersionVector) -> Option<Ordering> {
        self.0.read().unwrap().partial_cmp(&other.0.read().unwrap())
    }

    pub fn encode(&self) -> Vec<u8> {
        self.0.read().unwrap().encode()
    }

    pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
        let ans = Self(RwLock::new(loro::VersionVector::decode(bytes)?));
        Ok(ans)
    }
}

impl PartialEq for VersionVector {
    fn eq(&self, other: &Self) -> bool {
        self.0.read().unwrap().eq(&other.0.read().unwrap())
    }
}

impl Eq for VersionVector {}

#[derive(Debug)]
pub struct Frontiers(loro::Frontiers);

impl Frontiers {
    pub fn new() -> Self {
        Self(loro::Frontiers::default())
    }

    pub fn from_id(id: ID) -> Self {
        Self(loro::Frontiers::from(id))
    }

    pub fn from_ids(ids: Vec<ID>) -> Self {
        Self(loro::Frontiers::from(ids))
    }

    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
        let ans = Self(loro::Frontiers::decode(bytes)?);
        Ok(ans)
    }
}

impl PartialEq for Frontiers {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl Eq for Frontiers {}

impl Default for Frontiers {
    fn default() -> Self {
        Self::new()
    }
}

pub struct VersionVectorDiff {
    /// need to add these spans to move from right to left
    pub retreat: HashMap<PeerID, CounterSpan>,
    /// need to add these spans to move from left to right
    pub forward: HashMap<PeerID, CounterSpan>,
}

impl From<loro::VersionVectorDiff> for VersionVectorDiff {
    fn from(value: loro::VersionVectorDiff) -> Self {
        Self {
            retreat: value.retreat.into_iter().collect(),
            forward: value.forward.into_iter().collect(),
        }
    }
}

impl From<VersionVector> for loro::VersionVector {
    fn from(value: VersionVector) -> Self {
        value.0.into_inner().unwrap()
    }
}

impl From<&VersionVector> for loro::VersionVector {
    fn from(value: &VersionVector) -> Self {
        value.0.read().unwrap().clone()
    }
}

impl From<loro::VersionVector> for VersionVector {
    fn from(value: loro::VersionVector) -> Self {
        Self(RwLock::new(value))
    }
}

impl From<loro::Frontiers> for Frontiers {
    fn from(value: loro::Frontiers) -> Self {
        Self(value)
    }
}

impl From<Frontiers> for loro::Frontiers {
    fn from(value: Frontiers) -> Self {
        value.0
    }
}

impl From<&Frontiers> for loro::Frontiers {
    fn from(value: &Frontiers) -> Self {
        value.0.clone()
    }
}
