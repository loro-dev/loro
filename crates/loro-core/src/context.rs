use std::sync::{Arc, Mutex};

use crate::{
    container::{registry::ContainerInstance, ContainerID},
    LogStore,
};

pub trait Context {
    fn log_store(&self) -> &LogStore;
    fn log_store_mut(&mut self) -> &mut LogStore;
    fn get_container(&self, id: ContainerID) -> Arc<Mutex<ContainerInstance>>;
}
