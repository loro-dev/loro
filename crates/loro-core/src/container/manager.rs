use std::pin::Pin;

use fxhash::FxHashMap;

use crate::LogStore;

use super::{map::MapContainer, Container, ContainerID, ContainerType};

#[derive(Debug, Default)]
pub(crate) struct ContainerManager {
    containers: FxHashMap<ContainerID, Box<dyn Container>>,
}

impl ContainerManager {
    #[inline]
    pub fn create(
        &mut self,
        id: ContainerID,
        container_type: ContainerType,
        store: Pin<&mut LogStore>,
    ) -> Box<dyn Container> {
        match container_type {
            ContainerType::Map => Box::new(MapContainer::new(id, store)),
            _ => unimplemented!(),
        }
    }

    #[inline]
    pub fn get(&self, id: ContainerID) -> Option<&dyn Container> {
        self.containers.get(&id).map(|c| c.as_ref())
    }

    #[inline]
    pub fn get_mut(&mut self, id: ContainerID) -> Option<&mut Box<dyn Container>> {
        self.containers.get_mut(&id)
    }

    #[inline]
    fn insert(&mut self, id: ContainerID, container: Box<dyn Container>) {
        self.containers.insert(id, container);
    }

    pub fn get_or_create(
        &mut self,
        id: ContainerID,
        container_type: ContainerType,
        store: Pin<&mut LogStore>,
    ) -> &mut Box<dyn Container> {
        if !self.containers.contains_key(&id) {
            let container = self.create(id.clone(), container_type, store);
            self.insert(id.clone(), container);
        }

        self.get_mut(id).unwrap()
    }
}
