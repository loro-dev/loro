use std::sync::{Arc, RwLock};

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
    op::RemoteOp,
    LogStore, LoroError, VersionVector,
};

pub struct LoroCore {
    pub(crate) log_store: Arc<RwLock<LogStore>>,
    pub(crate) container: Arc<RwLock<ContainerManager>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        let container = Arc::new(RwLock::new(ContainerManager::new()));
        let weak = Arc::downgrade(&container);
        Self {
            log_store: LogStore::new(cfg, client_id, weak),
            container,
        }
    }

    pub fn vv(&self) -> VersionVector {
        self.log_store.read().unwrap().get_vv().clone()
    }

    #[inline(always)]
    pub fn get_or_create_root_map(
        &mut self,
        name: &str,
    ) -> Result<ContainerRefMut<MapContainer>, LoroError> {
        let mut a = OwningRefMut::new(self.container.write().unwrap());
        let id = ContainerID::new_root(name, ContainerType::Map);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container_idx(&id);
        let ptr = Arc::downgrade(&self.log_store);
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
        let mut a = OwningRefMut::new(self.container.write().unwrap());
        let id = ContainerID::new_root(name, ContainerType::Text);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container_idx(&id);
        let ptr = Arc::downgrade(&self.log_store);
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
        let a = OwningRefMut::new(self.container.write().unwrap());
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
        let a = OwningRefMut::new(self.container.write().unwrap());
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
        let a = OwningRef::new(self.container.read().unwrap());
        Ok(a.map(move |x| x.get(id).unwrap().as_text().unwrap()).into())
    }

    pub fn export(&self, remote_vv: VersionVector) -> Vec<Change<RemoteOp>> {
        let store = self.log_store.read().unwrap();
        store.export(&remote_vv)
    }

    pub fn import(&mut self, changes: Vec<Change<RemoteOp>>) {
        let mut store = self.log_store.write().unwrap();
        store.import(changes)
    }

    #[cfg(feature = "fuzzing")]
    pub fn debug_inspect(&self) {
        self.log_store.write().unwrap().debug_inspect();
        self.container.write().unwrap().debug_inspect();
    }
}
