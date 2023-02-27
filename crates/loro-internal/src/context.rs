use std::{
    cell::RefCell,
    sync::{Arc, Mutex, RwLock, Weak},
};

use crate::{
    container::{registry::ContainerInstance, ContainerID},
    hierarchy::Hierarchy,
    transaction::Transaction,
    LogStore, LoroCore,
};

pub trait Context {
    fn log_store(&self) -> Arc<RwLock<LogStore>>;
    fn hierarchy(&self) -> Arc<Mutex<Hierarchy>>;
    fn get_container(&self, id: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>>;
}

impl Context for LoroCore {
    fn log_store(&self) -> Arc<RwLock<LogStore>> {
        self.log_store.clone()
    }

    fn hierarchy(&self) -> Arc<Mutex<Hierarchy>> {
        self.hierarchy.clone()
    }

    fn get_container(&self, id: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>> {
        self.log_store.try_read().unwrap().get_container(id)
    }
}

impl<T> Context for RefCell<T>
where
    T: Context,
{
    fn log_store(&self) -> Arc<RwLock<LogStore>> {
        self.borrow().log_store()
    }

    fn hierarchy(&self) -> Arc<Mutex<Hierarchy>> {
        self.borrow().hierarchy()
    }

    fn get_container(&self, id: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>> {
        self.borrow().get_container(id)
    }
}
