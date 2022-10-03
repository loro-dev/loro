use std::{pin::Pin};

use crate::{
    configure::Configure,
    container::{map::MapContainer, Cast, Container, ContainerID, ContainerType},
    id::ClientID,
    InternalString, LogStore,
};

pub struct LoroCore {
    pub store: Pin<Box<LogStore>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        Self {
            store: LogStore::new(cfg, client_id),
        }
    }

    pub fn get_container(
        &mut self,
        name: InternalString,
        container: ContainerType,
    ) -> &mut dyn Container {
        self.store
            .container
            .get_or_create(&ContainerID::new_root(name, container))
    }

    pub fn get_map_container(&mut self, name: InternalString) -> Option<&mut MapContainer> {
        let a = self.get_container(name, ContainerType::Map);
        a.cast_mut()
    }
}
