use loro_internal::{id::PeerID, LoroDoc};

use crate::raw_store::RawStore;

pub struct Loro {
    pub this_client_id: PeerID,
    pub raw_store: Option<RawStore>,
    pub log_store: Option<LoroDoc>,
}
