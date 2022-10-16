use std::fmt::Display;

use serde::Serialize;

pub type ClientID = u64;
pub type Counter = i32;
const UNKNOWN: ClientID = 404;

#[derive(PartialEq, Eq, Hash, Clone, Debug, Copy, Serialize)]
pub struct ID {
    pub client_id: ClientID,
    pub counter: Counter,
}

impl Display for ID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.client_id, self.counter)
    }
}

impl PartialOrd for ID {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.client_id.partial_cmp(&other.client_id) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self.counter.partial_cmp(&other.counter)
    }
}

impl Ord for ID {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.client_id.cmp(&other.client_id) {
            core::cmp::Ordering::Equal => self.counter.cmp(&other.counter),
            ord => ord,
        }
    }
}

pub const ROOT_ID: ID = ID {
    client_id: u64::MAX,
    counter: i32::MAX,
};

impl From<u128> for ID {
    fn from(id: u128) -> Self {
        ID {
            client_id: (id >> 64) as ClientID,
            counter: id as Counter,
        }
    }
}

impl ID {
    #[inline]
    pub fn new(client_id: u64, counter: Counter) -> Self {
        ID { client_id, counter }
    }

    #[inline]
    pub fn new_root() -> Self {
        ROOT_ID
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.client_id == u64::MAX
    }

    #[inline]
    pub fn unknown(counter: Counter) -> Self {
        ID {
            client_id: UNKNOWN,
            counter,
        }
    }

    #[inline]
    pub fn is_unknown(&self) -> bool {
        self.client_id == UNKNOWN
    }

    #[inline]
    pub(crate) fn is_connected_id(&self, other: &Self, self_len: usize) -> bool {
        self.client_id == other.client_id && self.counter + self_len as Counter == other.counter
    }

    #[inline]
    pub fn inc(&self, inc: i32) -> Self {
        ID {
            client_id: self.client_id,
            counter: self.counter + inc,
        }
    }

    #[inline]
    pub fn contains(&self, len: Counter, target: ID) -> bool {
        self.client_id == target.client_id
            && self.counter <= target.counter
            && target.counter < self.counter + len
    }
}
