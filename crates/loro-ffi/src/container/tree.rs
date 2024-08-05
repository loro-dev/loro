#[derive(Debug, Clone)]
pub struct LoroTree {
    pub(crate) tree: loro::LoroTree,
}

impl LoroTree {
    pub fn new() -> Self {
        Self {
            tree: loro::LoroTree::new(),
        }
    }
}
