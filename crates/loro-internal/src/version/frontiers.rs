use super::*;
use either::Either;

/// Frontiers representation.
//
// Internal Invariance:
// - Frontiers::Map(map) always have at least 2 elements.
#[derive(Clone, Default)]
pub enum Frontiers {
    #[default]
    None,
    ID(ID),
    // We use internal map to avoid the module outside accidentally create or modify the map
    // to make it empty or only contain 1 element
    Map(InternalMap),
}

use std::{fmt, sync::Arc};

impl fmt::Debug for Frontiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Frontiers")
            .field(&FrontiersDebugHelper(self))
            .finish()
    }
}

struct FrontiersDebugHelper<'a>(&'a Frontiers);

impl fmt::Debug for FrontiersDebugHelper<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        match self.0 {
            Frontiers::None => {}
            Frontiers::ID(id) => {
                list.entry(id);
            }
            Frontiers::Map(map) => {
                for id in map.iter() {
                    list.entry(&id);
                }
            }
        }
        list.finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalMap(Arc<FxHashMap<PeerID, Counter>>);

impl InternalMap {
    fn new() -> Self {
        Self(Arc::new(FxHashMap::default()))
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn iter(&self) -> impl Iterator<Item = ID> + '_ {
        self.0
            .iter()
            .map(|(&peer, &counter)| ID::new(peer, counter))
    }

    fn contains(&self, id: &ID) -> bool {
        self.0
            .get(&id.peer)
            .is_some_and(|&counter| counter == id.counter)
    }

    fn insert(&mut self, id: ID) {
        Arc::make_mut(&mut self.0)
            .entry(id.peer)
            .and_modify(|e| *e = (*e).max(id.counter))
            .or_insert(id.counter);
    }

    fn remove(&mut self, id: &ID) -> bool {
        let map = Arc::make_mut(&mut self.0);
        if let Some(counter) = map.get_mut(&id.peer) {
            if *counter == id.counter {
                map.remove(&id.peer);
                return true;
            }
        }
        false
    }

    fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&ID) -> bool,
    {
        let map = Arc::make_mut(&mut self.0);
        map.retain(|&peer, &mut counter| f(&ID::new(peer, counter)));
    }
}

impl Frontiers {
    pub fn len(&self) -> usize {
        match self {
            Frontiers::None => 0,
            Frontiers::ID(_) => 1,
            Frontiers::Map(map) => map.len(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = ID> + '_ {
        match self {
            Frontiers::None => Either::Left(Either::Left(std::iter::empty())),
            Frontiers::ID(id) => Either::Left(Either::Right(std::iter::once(*id))),
            Frontiers::Map(map) => Either::Right(map.iter()),
        }
    }

    pub fn contains(&self, id: &ID) -> bool {
        match self {
            Frontiers::None => false,
            Frontiers::ID(inner_id) => inner_id == id,
            Frontiers::Map(map) => map.contains(id),
        }
    }

    pub fn push(&mut self, id: ID) {
        match self {
            Frontiers::None => *self = Frontiers::ID(id),
            Frontiers::ID(existing_id) => {
                if existing_id.peer != id.peer {
                    let mut map = InternalMap::new();
                    map.insert(*existing_id);
                    map.insert(id);
                    *self = Frontiers::Map(map);
                } else if existing_id.counter < id.counter {
                    *existing_id = id;
                }
            }
            Frontiers::Map(map) => map.insert(id),
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&ID) -> bool,
    {
        match self {
            Frontiers::None => {}
            Frontiers::ID(id) => {
                if !f(id) {
                    *self = Frontiers::None;
                }
            }
            Frontiers::Map(map) => {
                map.retain(|id| f(id));
                match map.len() {
                    0 => *self = Frontiers::None,
                    1 => {
                        let id = map.iter().next().unwrap();
                        *self = Frontiers::ID(id);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn remove(&mut self, id: &ID) {
        match self {
            Frontiers::None => {}
            Frontiers::ID(existing_id) => {
                if existing_id == id {
                    *self = Frontiers::None;
                }
            }
            Frontiers::Map(map) => {
                if map.remove(id) {
                    match map.len() {
                        0 => *self = Frontiers::None,
                        1 => {
                            let id = map.iter().next().unwrap();
                            *self = Frontiers::ID(id);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

impl PartialEq for Frontiers {
    fn eq(&self, other: &Self) -> bool {
        let len = self.len();
        if len != other.len() {
            return false;
        }

        match (self, other) {
            (Frontiers::None, Frontiers::None) => true,
            (Frontiers::ID(id1), Frontiers::ID(id2)) => id1 == id2,
            (Frontiers::Map(map1), Frontiers::Map(map2)) => map1 == map2,
            _ => unreachable!(),
        }
    }
}

impl Eq for Frontiers {}

impl Frontiers {
    pub fn new() -> Self {
        Self::None
    }

    #[inline]
    pub fn from_id(id: ID) -> Self {
        Self::ID(id)
    }

    #[inline]
    pub fn encode(&self) -> Vec<u8> {
        let mut vec: Vec<ID> = self.iter().collect();
        vec.sort();
        postcard::to_allocvec(&vec).unwrap()
    }

    #[inline]
    pub fn decode(bytes: &[u8]) -> Result<Self, LoroError> {
        let vec: Vec<ID> = postcard::from_bytes(bytes).map_err(|_| {
            LoroError::DecodeError("Decode Frontiers error".to_string().into_boxed_str())
        })?;
        Ok(Self::from(vec))
    }

    pub fn update_frontiers_on_new_change(&mut self, id: ID, deps: &Frontiers) {
        if self.len() <= 8 && self == deps {
            *self = Frontiers::from_id(id);
            return;
        }

        // Remove all IDs in deps from self
        for dep in deps.iter() {
            self.remove(&dep);
        }

        // Add the new ID
        self.push(id);
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the single ID if the Frontiers contains exactly one ID, otherwise returns None.
    pub fn as_single(&self) -> Option<ID> {
        match self {
            Frontiers::ID(id) => Some(*id),
            _ => None,
        }
    }

    /// Returns a reference to the internal map if the Frontiers contains multiple IDs,
    /// otherwise returns None.
    pub fn as_map(&self) -> Option<&InternalMap> {
        match self {
            Frontiers::Map(map) => Some(map),
            _ => None,
        }
    }

    /// Merges another Frontiers into this one.
    ///
    /// Id from other will override the id with the same peer from self.
    pub fn merge_with_greater(&mut self, other: &Frontiers) {
        if self.is_empty() {
            *self = other.clone();
            return;
        }

        if let Some(id) = self.as_single() {
            match other {
                Frontiers::None => {}
                Frontiers::ID(other_id) => {
                    if id.peer == other_id.peer {
                        *self = Frontiers::ID(ID::new(id.peer, id.counter.max(other_id.counter)));
                    } else {
                        self.push(*other_id);
                    }
                    return;
                }
                Frontiers::Map(internal_map) => {
                    let mut map = internal_map.clone();
                    Arc::make_mut(&mut map.0)
                        .entry(id.peer)
                        .and_modify(|c| *c = (*c).max(id.counter))
                        .or_insert(id.counter);
                    *self = Frontiers::Map(map);
                }
            }

            return;
        }

        let Frontiers::Map(map) = self else {
            unreachable!()
        };
        let map = Arc::make_mut(&mut map.0);
        for id in other.iter() {
            map.entry(id.peer)
                .and_modify(|c| *c = (*c).max(id.counter))
                .or_insert(id.counter);
        }
    }

    pub fn to_vec(&self) -> Vec<ID> {
        match self {
            Frontiers::None => Vec::new(),
            Frontiers::ID(id) => vec![*id],
            Frontiers::Map(map) => map.iter().collect(),
        }
    }

    /// Keeps only one element in the Frontiers, deleting all others.
    /// If the Frontiers is empty, it remains empty.
    /// If it contains multiple elements, it keeps the first one encountered.
    pub fn keep_one(&mut self) {
        match self {
            Frontiers::None => {}
            Frontiers::ID(_) => {}
            Frontiers::Map(map) => {
                if let Some((&peer, &counter)) = map.0.iter().next() {
                    *self = Frontiers::ID(ID::new(peer, counter));
                }
            }
        }
    }
}
impl From<&[ID]> for Frontiers {
    fn from(ids: &[ID]) -> Self {
        match ids.len() {
            0 => Frontiers::None,
            1 => Frontiers::ID(ids[0]),
            _ => {
                let mut map = InternalMap::new();
                for &id in ids {
                    map.insert(id);
                }
                Frontiers::Map(map)
            }
        }
    }
}

impl From<Vec<ID>> for Frontiers {
    fn from(ids: Vec<ID>) -> Self {
        match ids.len() {
            0 => Frontiers::None,
            1 => Frontiers::ID(ids[0]),
            _ => {
                let mut map = InternalMap::new();
                for id in ids {
                    map.insert(id);
                }
                Frontiers::Map(map)
            }
        }
    }
}

impl From<ID> for Frontiers {
    fn from(value: ID) -> Self {
        Self::ID(value)
    }
}

impl FromIterator<ID> for Frontiers {
    fn from_iter<I: IntoIterator<Item = ID>>(iter: I) -> Self {
        let mut new = Self::new();
        for id in iter {
            new.push(id);
        }
        new
    }
}

impl From<Option<ID>> for Frontiers {
    fn from(value: Option<ID>) -> Self {
        match value {
            Some(id) => Frontiers::ID(id),
            None => Frontiers::None,
        }
    }
}

impl<const N: usize> From<[ID; N]> for Frontiers {
    fn from(value: [ID; N]) -> Self {
        match N {
            0 => Frontiers::None,
            1 => Frontiers::ID(value[0]),
            _ => {
                let mut map = InternalMap::new();
                for id in value {
                    map.insert(id);
                }
                Frontiers::Map(map)
            }
        }
    }
}

impl From<&Vec<ID>> for Frontiers {
    fn from(ids: &Vec<ID>) -> Self {
        match ids.len() {
            0 => Frontiers::None,
            1 => Frontiers::ID(ids[0]),
            _ => {
                let mut map = InternalMap::new();
                for id in ids {
                    map.insert(*id);
                }
                Frontiers::Map(map)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontiers_push_insert_remove() {
        let mut frontiers = Frontiers::None;

        // Test push
        frontiers.push(ID::new(1, 1));
        assert_eq!(frontiers, Frontiers::ID(ID::new(1, 1)));

        frontiers.push(ID::new(2, 1));
        assert!(matches!(frontiers, Frontiers::Map(_)));
        assert_eq!(frontiers.len(), 2);

        frontiers.push(ID::new(1, 2));
        assert_eq!(frontiers.len(), 2);
        assert!(frontiers.contains(&ID::new(1, 2)));
        assert!(!frontiers.contains(&ID::new(1, 1)));

        // Test insert (via InternalMap)
        if let Frontiers::Map(ref mut map) = frontiers {
            map.insert(ID::new(3, 1));
        }
        assert_eq!(frontiers.len(), 3);
        assert!(frontiers.contains(&ID::new(3, 1)));

        // Test remove
        frontiers.remove(&ID::new(2, 1));
        assert_eq!(frontiers.len(), 2);
        assert!(!frontiers.contains(&ID::new(2, 1)));

        frontiers.remove(&ID::new(1, 2));
        assert_eq!(frontiers, Frontiers::ID(ID::new(3, 1)));

        frontiers.remove(&ID::new(3, 1));
        assert_eq!(frontiers, Frontiers::None);
    }

    #[test]
    fn test_frontiers_edge_cases() {
        let mut frontiers = Frontiers::None;

        // Push to empty
        frontiers.push(ID::new(1, 1));
        assert_eq!(frontiers, Frontiers::ID(ID::new(1, 1)));

        // Push same peer, higher counter
        frontiers.push(ID::new(1, 2));
        assert_eq!(frontiers, Frontiers::ID(ID::new(1, 2)));

        // Push same peer, lower counter (should not change)
        frontiers.push(ID::new(1, 1));
        assert_eq!(frontiers, Frontiers::ID(ID::new(1, 2)));

        // Push different peer
        frontiers.push(ID::new(2, 1));
        assert!(matches!(frontiers, Frontiers::Map(_)));
        assert_eq!(frontiers.len(), 2);

        // Remove non-existent
        frontiers.remove(&ID::new(3, 1));
        assert_eq!(frontiers.len(), 2);

        // Remove until only one left
        frontiers.remove(&ID::new(2, 1));
        assert_eq!(frontiers, Frontiers::ID(ID::new(1, 2)));

        // Remove last
        frontiers.remove(&ID::new(1, 2));
        assert_eq!(frontiers, Frontiers::None);
    }

    #[test]
    fn test_frontiers_retain() {
        let mut frontiers = Frontiers::None;

        // Test retain on empty frontiers
        frontiers.retain(|_| true);
        assert_eq!(frontiers, Frontiers::None);

        // Test retain on single ID
        frontiers.push(ID::new(1, 1));
        frontiers.retain(|id| id.peer == 1);
        assert_eq!(frontiers, Frontiers::ID(ID::new(1, 1)));

        frontiers.retain(|id| id.peer == 2);
        assert_eq!(frontiers, Frontiers::None);

        // Test retain on multiple IDs
        frontiers.push(ID::new(1, 1));
        frontiers.push(ID::new(2, 2));
        frontiers.push(ID::new(3, 3));

        // Retain only even peer IDs
        frontiers.retain(|id| id.peer % 2 == 0);
        assert_eq!(frontiers, Frontiers::ID(ID::new(2, 2)));

        // Add more IDs and test retaining multiple
        frontiers.push(ID::new(1, 1));
        frontiers.push(ID::new(3, 3));
        frontiers.push(ID::new(4, 4));

        frontiers.retain(|id| id.peer > 2);
        assert!(matches!(frontiers, Frontiers::Map(_)));
        assert_eq!(frontiers.len(), 2);
        assert!(frontiers.contains(&ID::new(3, 3)));
        assert!(frontiers.contains(&ID::new(4, 4)));

        // Retain none
        frontiers.retain(|_| false);
        assert_eq!(frontiers, Frontiers::None);
    }

    #[test]
    fn test_frontiers_encode_decode() {
        let mut frontiers = Frontiers::None;

        // Test encode/decode for empty frontiers
        let encoded = frontiers.encode();
        let decoded = Frontiers::decode(&encoded).unwrap();
        assert_eq!(frontiers, decoded);

        // Test encode/decode for single ID
        frontiers.push(ID::new(1, 100));
        let encoded = frontiers.encode();
        let decoded = Frontiers::decode(&encoded).unwrap();
        assert_eq!(frontiers, decoded);

        // Test encode/decode for multiple IDs
        frontiers.push(ID::new(2, 200));
        frontiers.push(ID::new(3, 300));
        let encoded = frontiers.encode();
        let decoded = Frontiers::decode(&encoded).unwrap();
        assert_eq!(frontiers, decoded);

        // Test encode/decode for many IDs
        for i in 4..20 {
            frontiers.push(ID::new(i, i as Counter * 100));
        }
        let encoded = frontiers.encode();
        let decoded = Frontiers::decode(&encoded).unwrap();
        assert_eq!(frontiers, decoded);

        // Test decode with invalid input
        assert!(Frontiers::decode(&[0xFF]).is_err());
        assert!(Frontiers::decode(&[]).is_err());
    }
}
