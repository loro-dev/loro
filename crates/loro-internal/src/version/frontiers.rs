use super::*;
use either::Either;

#[derive(Debug, Clone, Default)]
pub struct Frontiers(Inner);

/// Inner representation of Frontiers.
/// Invariants:
/// - If it contains 0 elements, it's None
/// - If it contains 1 element, it's ID
/// - If it contains 2 or more elements, it's Map
#[derive(Debug, Clone, Default)]
enum Inner {
    #[default]
    None,
    ID(ID),
    Map(FxHashMap<PeerID, Counter>),
}

impl Inner {
    fn len(&self) -> usize {
        match self {
            Inner::None => 0,
            Inner::ID(_) => 1,
            Inner::Map(map) => map.len(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = ID> + '_ {
        match self {
            Inner::None => Either::Left(Either::Left(std::iter::empty())),
            Inner::ID(id) => Either::Left(Either::Right(std::iter::once(*id))),
            Inner::Map(map) => {
                Either::Right(map.iter().map(|(&peer, &counter)| ID::new(peer, counter)))
            }
        }
    }

    fn contains(&self, id: &ID) -> bool {
        match self {
            Inner::None => false,
            Inner::ID(inner_id) => inner_id == id,
            Inner::Map(map) => map
                .get(&id.peer)
                .map_or(false, |&counter| counter == id.counter),
        }
    }

    fn push(&mut self, id: ID) {
        match self {
            Inner::None => *self = Inner::ID(id),
            Inner::ID(existing_id) => {
                if *existing_id != id {
                    let mut map = FxHashMap::default();
                    map.insert(existing_id.peer, existing_id.counter);
                    map.insert(id.peer, id.counter);
                    *self = Inner::Map(map);
                }
            }
            Inner::Map(map) => {
                map.entry(id.peer)
                    .and_modify(|counter| *counter = (*counter).max(id.counter))
                    .or_insert(id.counter);
            }
        }
    }

    fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&ID) -> bool,
    {
        match self {
            Inner::None => {}
            Inner::ID(id) => {
                if !f(id) {
                    *self = Inner::None;
                }
            }
            Inner::Map(map) => {
                map.retain(|&peer, &mut counter| f(&ID::new(peer, counter)));
                match map.len() {
                    0 => *self = Inner::None,
                    1 => {
                        let (&peer, &counter) = map.iter().next().unwrap();
                        *self = Inner::ID(ID::new(peer, counter));
                    }
                    _ => {}
                }
            }
        }
    }

    fn remove(&mut self, id: &ID) {
        match self {
            Inner::None => {}
            Inner::ID(existing_id) => {
                if existing_id == id {
                    *self = Inner::None;
                }
            }
            Inner::Map(map) => {
                if let Some(counter) = map.get_mut(&id.peer) {
                    if *counter == id.counter {
                        map.remove(&id.peer);
                        match map.len() {
                            0 => *self = Inner::None,
                            1 => {
                                let (&peer, &counter) = map.iter().next().unwrap();
                                *self = Inner::ID(ID::new(peer, counter));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

impl PartialEq for Frontiers {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }

        match (&self.0, &other.0) {
            (Inner::None, Inner::None) => true,
            (Inner::ID(id1), Inner::ID(id2)) => id1 == id2,
            (Inner::Map(map1), Inner::Map(map2)) => map1 == map2,
            _ => unreachable!(),
        }
    }
}

impl Frontiers {
    #[inline]
    pub fn from_id(id: ID) -> Self {
        Self(Inner::ID(id))
    }

    #[inline]
    pub fn encode(&self) -> Vec<u8> {
        let vec: Vec<ID> = self.0.iter().collect();
        postcard::to_allocvec(&vec).unwrap()
    }

    #[inline]
    pub fn decode(bytes: &[u8]) -> Result<Self, LoroError> {
        let vec: Vec<ID> = postcard::from_bytes(bytes).map_err(|_| {
            LoroError::DecodeError("Decode Frontiers error".to_string().into_boxed_str())
        })?;
        Ok(Self::from(vec))
    }

    pub fn retain_non_included(&mut self, other: &Frontiers) {
        self.0.retain(|id| !other.0.contains(id));
    }

    pub fn update_frontiers_on_new_change(&mut self, id: ID, deps: &Frontiers) {
        if self.0.len() <= 8 && self == deps {
            *self = Frontiers::from_id(id);
            return;
        }

        // Remove all IDs in deps from self
        for dep in deps.0.iter() {
            self.0.remove(&dep);
        }

        // Add the new ID
        self.0.push(id);
    }

    pub fn filter_peer(&mut self, peer: PeerID) {
        self.0.retain(|id| id.peer != peer);
    }

    #[inline]
    pub(crate) fn with_capacity(_cap: usize) -> Frontiers {
        Self(Inner::default())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.0, Inner::None)
    }

    pub fn iter(&self) -> impl Iterator<Item = ID> + '_ {
        self.0.iter()
    }

    pub fn contains(&self, id: &ID) -> bool {
        self.0.contains(id)
    }

    pub fn push(&mut self, id: ID) {
        self.0.push(id);
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&ID) -> bool,
    {
        self.0.retain(f);
    }
}

impl From<Vec<ID>> for Frontiers {
    fn from(value: Vec<ID>) -> Self {
        let inner = match value.len() {
            0 => Inner::None,
            1 => Inner::ID(value[0]),
            _ => {
                let map = value.into_iter().map(|id| (id.peer, id.counter)).collect();
                Inner::Map(map)
            }
        };
        Self(inner)
    }
}

impl FromIterator<ID> for Frontiers {
    fn from_iter<I: IntoIterator<Item = ID>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<Vec<ID>>())
    }
}

// Implement other From traits as needed
