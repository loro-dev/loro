#[derive(Debug, Clone)]
pub struct LoroMap {
    pub(crate) map: loro::LoroMap,
}

impl LoroMap {
    pub fn new() -> Self {
        Self {
            map: loro::LoroMap::new(),
        }
    }
}
