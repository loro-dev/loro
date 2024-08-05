#[derive(Debug, Clone)]
pub struct LoroCounter {
    pub(crate) counter: loro::LoroCounter,
}

impl LoroCounter {
    pub fn new() -> Self {
        Self {
            counter: loro::LoroCounter::new(),
        }
    }
}
