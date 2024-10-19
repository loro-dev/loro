use crate::ContainerID;

#[derive(Debug, Clone)]
pub struct LoroUnknown {
    pub(crate) unknown: loro::LoroUnknown,
}

impl LoroUnknown {
    pub fn id(&self) -> ContainerID {
        self.unknown.id().into()
    }
}
