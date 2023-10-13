use loro_common::LoroValue;

use crate::InternalString;

use super::Meta;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StyleMetaItem {
    pub style_key: InternalString,
    pub value: LoroValue,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StyleMeta {
    pub vec: Vec<StyleMetaItem>,
}

impl Meta for StyleMeta {
    fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    fn compose(&mut self, other: &Self, type_pair: (super::DeltaType, super::DeltaType)) {
        unimplemented!()
    }

    fn is_mergeable(&self, other: &Self) -> bool {
        true
    }

    fn merge(&mut self, other: &Self) {
        self.vec.extend_from_slice(&other.vec)
    }
}
