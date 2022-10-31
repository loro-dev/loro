use std::ptr::NonNull;

use owning_ref::{OwningRef, OwningRefMut};

use crate::{
    change::Change,
    configure::Configure,
    container::{
        manager::{ContainerManager, ContainerRef, ContainerRefMut},
        map::MapContainer,
        text::text_container::TextContainer,
        ContainerID, ContainerType,
    },
    id::ClientID,
    isomorph::{Irc, IsoRefMut, IsoRw},
    LogStore, LoroError, VersionVector,
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

    #[inline(always)]
    pub fn get_or_create_root_map(
        &mut self,
        name: &str,
    ) -> Result<ContainerRefMut<MapContainer>, LoroError> {
        let mut a = OwningRefMut::new(self.container.write());
        let id = ContainerID::new_root(name, ContainerType::Map);
        let ptr = Irc::downgrade(&self.log_store);
        a.get_or_create(&id, ptr)?;
        Ok(
            a.map_mut(move |x| x.get_mut(&id).unwrap().as_map_mut().unwrap())
                .into(),
        )
    }

    #[inline(always)]
    pub fn get_or_create_root_text(
        &mut self,
        name: &str,
    ) -> Result<ContainerRefMut<TextContainer>, LoroError> {
        let mut a = OwningRefMut::new(self.container.write());
        let id = ContainerID::new_root(name, ContainerType::Text);
        let ptr = Irc::downgrade(&self.log_store);
        a.get_or_create(&id, ptr)?;
        Ok(
            a.map_mut(move |x| x.get_mut(&id).unwrap().as_text_mut().unwrap())
                .into(),
        )
    }

    #[inline(always)]
    pub fn get_map_container_mut(
        &mut self,
        id: &ContainerID,
    ) -> Result<ContainerRefMut<MapContainer>, LoroError> {
        let a = OwningRefMut::new(self.container.write());
        Ok(
            a.map_mut(move |x| x.get_mut(id).unwrap().as_map_mut().unwrap())
                .into(),
        )
    }

    #[inline(always)]
    pub fn get_text_container_mut(
        &mut self,
        id: &ContainerID,
    ) -> Result<ContainerRefMut<TextContainer>, LoroError> {
        let a = OwningRefMut::new(self.container.write());
        Ok(
            a.map_mut(move |x| x.get_mut(id).unwrap().as_text_mut().unwrap())
                .into(),
        )
    }

    #[inline(always)]
    pub fn get_text_container(
        &self,
        id: &ContainerID,
    ) -> Result<ContainerRef<TextContainer>, LoroError> {
        let a = OwningRef::new(self.container.read());
        Ok(a.map(move |x| x.get(id).unwrap().as_text().unwrap()).into())
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
