use loro_internal::{id::ClientID, LoroCore};

use crate::raw_store::RawStore;

pub struct Loro {
    pub this_client_id: ClientID,
    pub raw_store: Option<RawStore>,
    pub log_store: Option<LoroCore>,
}
