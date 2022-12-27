use std::sync::{Arc, Mutex, RwLock, Weak};

use crate::{
    container::{registry::ContainerInstance, ContainerID},
    LogStore, LoroCore,
};

pub trait Context {
    fn log_store(&self) -> Arc<RwLock<LogStore>>;
    fn get_container(&self, id: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>>;
}

impl Context for LoroCore {
    fn log_store(&self) -> Arc<RwLock<LogStore>> {
        self.log_store.clone()
    }

    fn get_container(&self, id: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>> {
        self.log_store.try_read().unwrap().get_container(id)
    }
}
