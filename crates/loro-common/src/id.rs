use crate::{span::IdSpan, CounterSpan, IdFull, IdLp, IdLpSpan, Lamport};

use super::{Counter, LoroError, PeerID, ID};
const UNKNOWN: PeerID = 404;
use std::{
    fmt::{Debug, Display},
    ops::RangeBounds,
};

impl Debug for ID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{}@{}", self.counter, self.peer).as_str())
    }
}

impl Debug for IdLp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("L{}@{}", self.lamport, self.peer).as_str())
    }
}

impl Display for ID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{}@{}", self.counter, self.peer).as_str())
    }
}

impl Display for IdLp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("L{}@{}", self.lamport, self.peer).as_str())
    }
}

impl TryFrom<&str> for ID {
    type Error = LoroError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.split('@').count() != 2 {
            return Err(LoroError::DecodeError("Invalid ID format".into()));
        }

        let mut iter = value.split('@');
        let counter = iter
            .next()
            .unwrap()
            .parse::<Counter>()
            .map_err(|_| LoroError::DecodeError("Invalid ID format".into()))?;
        let client_id = iter
            .next()
            .unwrap()
            .parse::<u64>()
            .map_err(|_| LoroError::DecodeError("Invalid ID format".into()))?;
        Ok(ID {
            peer: client_id,
            counter,
        })
    }
}

impl PartialOrd for ID {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ID {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.peer.cmp(&other.peer) {
            core::cmp::Ordering::Equal => self.counter.cmp(&other.counter),
            ord => ord,
        }
    }
}

pub const ROOT_ID: ID = ID {
    peer: PeerID::MAX,
    counter: i32::MAX,
};

impl From<u128> for ID {
    fn from(id: u128) -> Self {
        ID {
            peer: (id >> 64) as PeerID,
            counter: id as Counter,
        }
    }
}

impl ID {
    /// The ID of the null object. This should be use rarely.
    pub const NONE_ID: ID = ID::new(u64::MAX, 0);

    #[inline]
    pub const fn new(peer: PeerID, counter: Counter) -> Self {
        ID { peer, counter }
    }

    #[inline]
    pub fn new_root() -> Self {
        ROOT_ID
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.peer == PeerID::MAX
    }

    #[inline]
    pub fn to_span(&self, len: usize) -> IdSpan {
        IdSpan {
            client_id: self.peer,
            counter: CounterSpan::new(self.counter, self.counter + len as Counter),
        }
    }

    #[inline]
    pub fn unknown(counter: Counter) -> Self {
        ID {
            peer: UNKNOWN,
            counter,
        }
    }

    #[inline]
    pub fn is_unknown(&self) -> bool {
        self.peer == UNKNOWN
    }

    #[inline]
    #[allow(dead_code)]
    pub(crate) fn is_connected_id(&self, other: &Self, self_len: usize) -> bool {
        self.peer == other.peer && self.counter + self_len as Counter == other.counter
    }

    #[inline]
    pub fn inc(&self, inc: i32) -> Self {
        ID {
            peer: self.peer,
            counter: self.counter + inc,
        }
    }

    #[inline]
    pub fn contains(&self, len: Counter, target: ID) -> bool {
        self.peer == target.peer
            && self.counter <= target.counter
            && target.counter < self.counter + len
    }
}

impl From<ID> for u128 {
    fn from(id: ID) -> Self {
        ((id.peer as u128) << 64) | id.counter as u128
    }
}

impl RangeBounds<ID> for (ID, ID) {
    fn start_bound(&self) -> std::ops::Bound<&ID> {
        std::ops::Bound::Included(&self.0)
    }

    fn end_bound(&self) -> std::ops::Bound<&ID> {
        std::ops::Bound::Excluded(&self.1)
    }
}

impl IdLp {
    pub const NONE_ID: IdLp = IdLp::new(u64::MAX, Lamport::MAX);

    #[inline]
    pub const fn new(peer: PeerID, lp: Lamport) -> Self {
        Self { peer, lamport: lp }
    }

    pub fn inc(&self, offset: i32) -> IdLp {
        IdLp {
            peer: self.peer,
            lamport: (self.lamport as i32 + offset) as Lamport,
        }
    }
}

impl From<IdLp> for IdLpSpan {
    fn from(value: IdLp) -> Self {
        IdLpSpan {
            peer: value.peer,
            lamport: crate::LamportSpan {
                start: value.lamport,
                end: value.lamport + 1,
            },
        }
    }
}

impl IdFull {
    pub const NONE_ID: IdFull = IdFull {
        peer: PeerID::MAX,
        lamport: 0,
        counter: 0,
    };

    pub fn new(peer: PeerID, counter: Counter, lamport: Lamport) -> Self {
        Self {
            peer,
            lamport,
            counter,
        }
    }

    pub fn inc(&self, offset: i32) -> IdFull {
        IdFull {
            peer: self.peer,
            lamport: (self.lamport as i32 + offset) as Lamport,
            counter: self.counter + offset as Counter,
        }
    }

    pub fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.counter,
        }
    }

    pub fn idlp(&self) -> IdLp {
        IdLp {
            peer: self.peer,
            lamport: self.lamport,
        }
    }
}
