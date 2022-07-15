use loro_core::{ClientID, LogStore};

use crate::raw_store::RawStore;

#[derive(Default)]
pub struct Loro {
    pub this_client_id: ClientID,
    pub raw_store: Option<RawStore>,
    pub log_store: Option<LogStore>,
}
