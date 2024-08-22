use std::sync::{Arc, Mutex};

use bytes::Bytes;
use fxhash::FxHashMap;
use loro_common::ContainerID;

use crate::kv_store::KvStore;

pub(crate) enum Status {
    BytesOnly,
    ImmBoth,
    MutState,
}

pub(crate) struct KvWrapper<S> {
    kv: Arc<Mutex<dyn KvStore>>,
    status: FxHashMap<ContainerID, Status>,
    _phantom: std::marker::PhantomData<S>,
}

pub(crate) trait KvState {
    fn from_bytes(bytes: Bytes) -> Self;
    fn to_bytes(&self) -> Bytes;
}
