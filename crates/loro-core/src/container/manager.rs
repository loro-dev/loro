use std::ptr::NonNull;

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;

use crate::{log_store::LogStoreWeakRef, span::IdSpan, LogStore};

use super::{
    map::MapContainer, text::text_container::TextContainer, Container, ContainerID, ContainerType,
};

#[derive(Debug, EnumAsInner)]
pub enum ContainerInstance {
    Map(Box<MapContainer>),
    Text(Box<TextContainer>),
    Dyn(Box<dyn Container>),
}

impl Container for ContainerInstance {
    fn id(&self) -> &ContainerID {
        match self {
            ContainerInstance::Map(x) => x.id(),
            ContainerInstance::Text(x) => x.id(),
            ContainerInstance::Dyn(x) => x.id(),
        }
    }

    fn type_(&self) -> ContainerType {
        match self {
            ContainerInstance::Map(_) => ContainerType::Map,
            ContainerInstance::Text(_) => ContainerType::Text,
            ContainerInstance::Dyn(x) => x.type_(),
        }
    }

    fn apply(&mut self, id_span: IdSpan, log: &LogStore) {
        match self {
            ContainerInstance::Map(x) => x.apply(id_span, log),
            ContainerInstance::Text(x) => x.apply(id_span, log),
            ContainerInstance::Dyn(x) => x.apply(id_span, log),
        }
    }

    fn checkout_version(&mut self, vv: &crate::VersionVector) {
        match self {
            ContainerInstance::Map(x) => x.checkout_version(vv),
            ContainerInstance::Text(x) => x.checkout_version(vv),
            ContainerInstance::Dyn(x) => x.checkout_version(vv),
        }
    }

    fn get_value(&mut self) -> &crate::LoroValue {
        match self {
            ContainerInstance::Map(x) => x.get_value(),
            ContainerInstance::Text(x) => x.get_value(),
            ContainerInstance::Dyn(x) => x.get_value(),
        }
    }

    fn to_export(&self, op: &mut crate::op::Op) {
        match self {
            ContainerInstance::Map(x) => x.to_export(op),
            ContainerInstance::Text(x) => x.to_export(op),
            ContainerInstance::Dyn(x) => x.to_export(op),
        }
    }
}

// TODO: containers snapshot: we need to resolve each container's parent even
// if its creation op is not in the logStore
#[derive(Debug)]
pub struct ContainerManager {
    pub(crate) containers: FxHashMap<ContainerID, ContainerInstance>,
    pub(crate) store: NonNull<LogStore>,
}

impl ContainerManager {
    #[inline]
    pub fn create(
        &mut self,
        id: ContainerID,
        container_type: ContainerType,
        log_store: LogStoreWeakRef,
    ) -> ContainerInstance {
        match container_type {
            ContainerType::Map => ContainerInstance::Map(Box::new(MapContainer::new(id))),
            ContainerType::Text => {
                ContainerInstance::Text(Box::new(TextContainer::new(id, log_store)))
            }
            _ => unimplemented!(),
        }
    }

    #[inline]
    pub fn get(&self, id: &ContainerID) -> Option<&ContainerInstance> {
        self.containers.get(id)
    }

    #[inline]
    pub fn get_mut(&mut self, id: &ContainerID) -> Option<&mut ContainerInstance> {
        self.containers.get_mut(id)
    }

    #[inline]
    fn insert(&mut self, id: ContainerID, container: ContainerInstance) {
        self.containers.insert(id, container);
    }

    pub fn get_or_create(
        &mut self,
        id: &ContainerID,
        log_store: LogStoreWeakRef,
    ) -> &mut ContainerInstance {
        if !self.containers.contains_key(id) {
            let container = self.create(id.clone(), id.container_type(), log_store);
            self.insert(id.clone(), container);
        }

        self.get_mut(id).unwrap()
    }
}
