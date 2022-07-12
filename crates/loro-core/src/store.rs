use std::collections::HashMap;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::id::ClientID;

#[non_exhaustive]
struct Change {}

struct Store {
    map: HashMap<ClientID, Vec<Change>>,
}
