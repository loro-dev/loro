use crate::ContainerID;

#[derive(Debug, Clone)]
pub struct LoroUnknown {
    pub(crate) inner: loro::LoroUnknown,
}

impl LoroUnknown {
    pub fn id(&self) -> ContainerID {
        self.inner.id().into()
    }
}
