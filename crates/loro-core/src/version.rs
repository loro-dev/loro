use fxhash::FxHashMap;

use crate::ClientID;

pub type VersionVector = FxHashMap<ClientID, u32>;
