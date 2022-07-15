use rle::RleVec;
use std::collections::HashMap;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::{change::Change, id::ClientID, Lamport, ID};

pub struct LogStore {
    ops: HashMap<ClientID, RleVec<Change>>,
    lamport: Lamport,
}

impl LogStore {
    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.ops
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }
}
