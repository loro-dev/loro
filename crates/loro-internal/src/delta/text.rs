use std::sync::Arc;

use fxhash::FxHashMap;
use loro_common::{InternalString, LoroValue, PeerID};
use loro_delta::delta_trait::DeltaAttr;
use serde::{Deserialize, Serialize};

use crate::change::Lamport;
use crate::container::richtext::{Style, Styles};
use crate::ToJson;

use super::Meta;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleMeta {
    map: FxHashMap<InternalString, StyleMetaItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleMetaItem {
    // We need lamport and peer to compose the event
    pub lamport: Lamport,
    pub peer: PeerID,
    pub value: LoroValue,
}

impl StyleMetaItem {
    pub fn try_replace(&mut self, other: &StyleMetaItem) {
        if (self.lamport, self.peer) < (other.lamport, other.peer) {
            self.lamport = other.lamport;
            self.peer = other.peer;
            self.value = other.value.clone();
        }
    }
}

impl From<&Styles> for StyleMeta {
    fn from(styles: &Styles) -> Self {
        let mut map = FxHashMap::with_capacity_and_hasher(styles.len(), Default::default());
        for (key, value) in styles.iter() {
            if let Some(value) = value.get() {
                map.insert(
                    key.key().clone(),
                    StyleMetaItem {
                        value: value.to_value(),
                        lamport: value.lamport,
                        peer: value.peer,
                    },
                );
            }
        }
        Self { map }
    }
}

impl From<Styles> for StyleMeta {
    fn from(styles: Styles) -> Self {
        let temp = &styles;
        temp.into()
    }
}

impl Meta for StyleMeta {
    fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn compose(&mut self, other: &Self, _type_pair: (super::DeltaType, super::DeltaType)) {
        for (key, value) in other.map.iter() {
            match self.map.get_mut(key) {
                Some(old_value) => {
                    old_value.try_replace(value);
                }
                None => {
                    self.map.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn is_mergeable(&self, other: &Self) -> bool {
        self.map == other.map
    }

    fn merge(&mut self, _: &Self) {}
}

impl StyleMeta {
    pub(crate) fn iter(&self) -> impl Iterator<Item = (InternalString, Style)> + '_ {
        self.map.iter().map(|(key, style)| {
            (
                key.clone(),
                Style {
                    key: key.clone(),
                    data: style.value.clone(),
                },
            )
        })
    }

    pub(crate) fn insert(&mut self, key: InternalString, value: StyleMetaItem) {
        self.map.insert(key, value);
    }

    pub(crate) fn contains_key(&self, key: &InternalString) -> bool {
        self.map.contains_key(key)
    }

    pub(crate) fn to_value(&self) -> LoroValue {
        LoroValue::Map(Arc::new(self.to_map_without_null_value()))
    }

    fn to_map_without_null_value(&self) -> FxHashMap<String, LoroValue> {
        self.map
            .iter()
            .filter_map(|(key, value)| {
                if value.value.is_null() {
                    None
                } else {
                    Some((key.to_string(), value.value.clone()))
                }
            })
            .collect()
    }

    pub(crate) fn to_map(&self) -> FxHashMap<String, LoroValue> {
        self.map
            .iter()
            .map(|(key, value)| (key.to_string(), value.value.clone()))
            .collect()
    }

    pub(crate) fn to_option_map(&self) -> Option<FxHashMap<String, LoroValue>> {
        if self.is_empty() {
            return None;
        }

        Some(self.to_map())
    }
}

impl ToJson for StyleMeta {
    fn to_json_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (key, style) in self.iter() {
            let value = serde_json::to_value(&style.data).unwrap();
            map.insert(key.to_string(), value);
        }

        serde_json::Value::Object(map)
    }

    fn from_json(_: &str) -> Self {
        unreachable!()
    }
}

impl DeltaAttr for StyleMeta {
    fn compose(&mut self, other: &Self) {
        for (key, value) in other.map.iter() {
            match self.map.get_mut(key) {
                Some(old_value) => {
                    old_value.try_replace(value);
                }
                None => {
                    self.map.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn attr_is_empty(&self) -> bool {
        self.is_empty()
    }
}
