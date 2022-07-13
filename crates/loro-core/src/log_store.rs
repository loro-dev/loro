use rle::RleVec;
use std::collections::HashMap;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::{change::Change, id::ClientID};

pub struct LogStore {
    map: HashMap<ClientID, RleVec<Change>>,
    lamport: usize,
}
