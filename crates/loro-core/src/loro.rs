use std::pin::Pin;

use crate::{configure::Configure, id::ClientID, LogStore};

pub struct LoroCore {
    pub store: Pin<Box<LogStore>>,
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        Self {
            store: LogStore::new(cfg, client_id),
        }
    }
}
