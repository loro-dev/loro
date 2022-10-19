use std::ptr::NonNull;

use fxhash::FxHashMap;

use crate::LogStore;

use super::{map::MapContainer, Container, ContainerID, ContainerType};

// TODO: containers snapshot: we need to resolve each container's parent even
// if its creation op is not in the logStore
#[derive(Debug)]
pub(crate) struct ContainerManager {
    pub(crate) containers: FxHashMap<ContainerID, Box<dyn Container>>,
    pub(crate) store: NonNull<LogStore>,
}

impl ContainerManager {
    #[inline]
    pub fn create(&mut self, id: ContainerID, container_type: ContainerType) -> Box<dyn Container> {
        match container_type {
            ContainerType::Map => Box::new(MapContainer::new(id)),
            _ => unimplemented!(),
        }
    }

    #[inline]
    pub fn get(&self, id: ContainerID) -> Option<&dyn Container> {
        self.containers.get(&id).map(|c| c.as_ref())
    }

    #[inline]
    pub fn get_mut(&mut self, id: &ContainerID) -> Option<&mut Box<dyn Container>> {
        self.containers.get_mut(id)
    }

    #[inline]
    fn insert(&mut self, id: ContainerID, container: Box<dyn Container>) {
        self.containers.insert(id, container);
    }

    pub fn get_or_create(&mut self, id: &ContainerID) -> &mut (dyn Container + 'static) {
        if !self.containers.contains_key(id) {
            let container = self.create(id.clone(), id.container_type());
            self.insert(id.clone(), container);
        }

        self.get_mut(id).unwrap().as_mut()
    }
}
