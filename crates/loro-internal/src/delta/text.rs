use std::sync::Arc;

use fxhash::FxHashMap;
use loro_common::{LoroValue, PeerID};
use serde::{Deserialize, Serialize};

use crate::change::Lamport;
use crate::container::richtext::{Style, StyleKey, Styles};
use crate::ToJson;

use super::Meta;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleMeta {
    map: FxHashMap<StyleKey, StyleMetaItem>,
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

impl From<Styles> for StyleMeta {
    fn from(styles: Styles) -> Self {
        let mut map = FxHashMap::with_capacity_and_hasher(styles.len(), Default::default());
        for (key, value) in styles.iter() {
            if let Some(value) = value.get() {
                map.insert(
                    key.clone(),
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
    pub(crate) fn iter(&self) -> impl Iterator<Item = (StyleKey, Style)> + '_ {
        self.map.iter().map(|(key, style)| {
            (
                key.clone(),
                Style {
                    key: key.key().clone(),
                    data: style.value.clone(),
                },
            )
        })
    }

    pub(crate) fn insert(&mut self, key: StyleKey, value: StyleMetaItem) {
        self.map.insert(key, value);
    }

    pub(crate) fn to_value(&self) -> LoroValue {
        LoroValue::Map(Arc::new(
            self.map
                .iter()
                .map(|(key, value)| {
                    (
                        key.to_attr_key(),
                        if key.contains_id() {
                            let mut map: FxHashMap<String, LoroValue> = Default::default();
                            map.insert("key".into(), key.key().to_string().into());
                            map.insert("data".into(), value.value.clone());
                            LoroValue::Map(Arc::new(map))
                        } else {
                            value.value.clone()
                        },
                    )
                })
                .collect(),
        ))
    }
}

impl ToJson for StyleMeta {
    fn to_json_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (key, style) in self.iter() {
            let value = if matches!(style.data, LoroValue::Null | LoroValue::Bool(_)) {
                serde_json::to_value(&style.data).unwrap()
            } else {
                let mut value = serde_json::Map::new();
                value.insert("key".to_string(), style.key.to_string().into());
                let data = serde_json::to_value(&style.data).unwrap();
                value.insert("data".to_string(), data);
                value.into()
            };
            map.insert(key.to_attr_key(), value);
        }

        serde_json::Value::Object(map)
    }

    fn from_json(_: &str) -> Self {
        unreachable!()
    }
}
