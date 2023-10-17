use crate::container::richtext::Style;

use super::Meta;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StyleMeta {
    pub vec: Vec<Style>,
}

impl Meta for StyleMeta {
    fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    fn compose(&mut self, other: &Self, type_pair: (super::DeltaType, super::DeltaType)) {}

    fn is_mergeable(&self, other: &Self) -> bool {
        self.vec == other.vec
    }

    fn merge(&mut self, other: &Self) {
        self.vec.extend_from_slice(&other.vec)
    }
}
