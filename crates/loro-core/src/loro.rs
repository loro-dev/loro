use std::ptr::NonNull;

use owning_ref::{OwningRef, OwningRefMut};

use crate::{
    change::Change,
    configure::Configure,
    container::{
        manager::{ContainerInstance, ContainerManager, ContainerRef},
        map::MapContainer,
        text::text_container::TextContainer,
        ContainerID, ContainerType,
    },
    id::ClientID,
    isomorph::{Irc, IsoRefMut, IsoRw},
    InternalString, LogStore, LogStore, VersionVector, VersionVector,
};

pub struct LoroCore {
    pub(crate) log_store: Irc<IsoRw<LogStore>>,
    pub(crate) container: Irc<IsoRw<ContainerManager>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        let container = Irc::new(IsoRw::new(ContainerManager {
            containers: Default::default(),
            store: NonNull::dangling(),
        }));
        let weak = Irc::downgrade(&container);
        Self {
            log_store: LogStore::new(cfg, client_id, weak),
            container,
        }
    }

    pub fn vv(&self) -> VersionVector {
        self.log_store.read().get_vv().clone()
    }

    pub fn get_container(
        &mut self,
        name: &str,
        container: ContainerType,
    ) -> OwningRefMut<IsoRefMut<ContainerManager>, ContainerInstance> {
        let a = OwningRefMut::new(self.container.write());
        a.map_mut(|x| {
            x.get_or_create(
                &ContainerID::new_root(name, container),
                Irc::downgrade(&self.log_store),
            )
        })
    }

    pub fn get_map_container(
        &mut self,
        name: &str,
    ) -> OwningRefMut<RwLockWriteGuard<ContainerManager>, Box<MapContainer>> {
        let a = OwningRefMut::new(self.container.write());
        a.map_mut(|x| {
            x.get_or_create(
                &ContainerID::new_root(name, ContainerType::Map),
                Irc::downgrade(&self.log_store),
            )
            .as_map_mut()
            .unwrap()
        })
    }

    pub fn get_or_create_text_container_mut(&mut self, name: &str) -> ContainerRef<TextContainer> {
        let a = OwningRefMut::new(self.container.write());
        a.map_mut(|x| {
            x.get_or_create(
                &ContainerID::new_root(name, ContainerType::Text),
                Irc::downgrade(&self.log_store),
            )
            .as_text_mut()
            .unwrap()
        })
        .into()
    }

    pub fn get_text_container(
        &self,
        name: &str,
    ) -> OwningRef<RwLockWriteGuard<ContainerManager>, Box<TextContainer>> {
        let a = OwningRef::new(self.container.write());
        a.map(|x| {
            x.get(&ContainerID::new_root(name, ContainerType::Text))
                .unwrap()
                .as_text()
                .unwrap()
        })
    }

    pub fn export(&self, remote_vv: VersionVector) -> Vec<Change> {
        let store = self.log_store.read();
        store.export(&remote_vv)
    }

    pub fn import(&mut self, changes: Vec<Change>) {
        let mut store = self.log_store.write();
        store.import(changes)
    }
}
