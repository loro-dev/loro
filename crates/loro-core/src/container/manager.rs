use std::{num::NonZeroI128, pin::Pin, ptr::NonNull, rc::Weak};

use fxhash::FxHashMap;

use crate::LogStore;

use super::{map::MapContainer, Container, ContainerID, ContainerType};

// TODO: containers snapshot: we need to resolve each container's parent even
// if its creation op is not in the logStore
#[derive(Debug)]
pub(crate) struct ContainerManager {
    pub(crate) containers: FxHashMap<ContainerID, Pin<Box<dyn Container>>>,
    pub(crate) store: NonNull<LogStore>,
}

impl ContainerManager {
    #[inline]
    pub fn create(
        &mut self,
        id: ContainerID,
        container_type: ContainerType,
        store: NonNull<LogStore>,
    ) -> Pin<Box<dyn Container>> {
        match container_type {
            ContainerType::Map => Box::pin(MapContainer::new(id, store)),
            _ => unimplemented!(),
        }
    }

    #[inline]
    pub fn get(&self, id: ContainerID) -> Option<Pin<&dyn Container>> {
        self.containers.get(&id).map(|c| c.as_ref())
    }

    #[inline]
    pub fn get_mut(&mut self, id: &ContainerID) -> Option<&mut Pin<Box<dyn Container>>> {
        self.containers.get_mut(id)
    }

    #[inline]
    fn insert(&mut self, id: ContainerID, container: Pin<Box<dyn Container>>) {
        self.containers.insert(id, container);
    }

    pub fn get_or_create(&mut self, id: &ContainerID) -> Pin<&mut dyn Container> {
        if !self.containers.contains_key(id) {
            let container = self.create(id.clone(), id.container_type(), self.store);
            self.insert(id.clone(), container);
        }

        self.get_mut(id).unwrap().as_mut()
    }
}
