#[derive(Debug, Clone)]
pub struct LoroMovableList {
    pub(crate) list: loro::LoroMovableList,
}

impl LoroMovableList {
    pub fn new() -> Self {
        Self {
            list: loro::LoroMovableList::new(),
        }
    }
}
