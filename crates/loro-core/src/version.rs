use fxhash::FxHashMap;

use crate::{change::Lamport, ClientID};

pub type VersionVector = FxHashMap<ClientID, u32>;

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub(crate) struct TotalOrderStamp {
    pub(crate) lamport: Lamport,
    pub(crate) client_id: ClientID,
}
