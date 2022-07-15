use loro_core::LogStore;

use crate::raw_store::RawStore;

#[derive(Default)]
pub struct Loro {
    pub raw_store: Option<RawStore>,
    pub log_store: Option<LogStore>,
}
