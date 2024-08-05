use std::fmt::Display;

use loro::LoroResult;

#[derive(Debug, Clone)]
pub struct LoroText {
    pub(crate) text: loro::LoroText,
}

impl LoroText {
    pub fn new() -> Self {
        Self {
            text: loro::LoroText::new(),
        }
    }

    pub fn insert(&self, pos: u32, text: &str) -> LoroResult<()> {
        self.text.insert(pos as usize, text)
    }
}

impl Display for LoroText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text.to_string())
    }
}

impl Default for LoroText {
    fn default() -> Self {
        Self::new()
    }
}
