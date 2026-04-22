use loro_common::{InternalString, LoroValue, PeerID};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::change::Lamport;
use crate::container::richtext::{Style, Styles};
use crate::event::TextMeta;
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

impl Meta for TextMeta {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn compose(&mut self, other: &Self, _: (super::DeltaType, super::DeltaType)) {
        for (key, value) in other.0.iter() {
            self.0.insert(key.clone(), value.clone());
        }
    }

    fn is_mergeable(&self, other: &Self) -> bool {
        self.0 == other.0
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
        LoroValue::Map(self.to_map_without_null_value().into())
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

    pub(crate) fn to_option_map_without_null_value(&self) -> Option<FxHashMap<String, LoroValue>> {
        let map = self.to_map_without_null_value();
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    }
}

impl ToJson for TextMeta {
    fn to_json_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (key, value) in self.0.iter() {
            let value = serde_json::to_value(value).unwrap();
            map.insert(key.to_string(), value);
        }

        serde_json::Value::Object(map)
    }

    fn from_json(s: &str) -> Self {
        let map: FxHashMap<String, LoroValue> = serde_json::from_str(s).unwrap();
        TextMeta(map)
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashMap;

    use super::*;
    use crate::delta::{DeltaType, Meta};

    fn key(value: &str) -> InternalString {
        value.into()
    }

    fn style_item(lamport: Lamport, peer: PeerID, value: LoroValue) -> StyleMetaItem {
        StyleMetaItem {
            lamport,
            peer,
            value,
        }
    }

    #[test]
    fn style_meta_item_replacement_is_ordered_by_lamport_then_peer() {
        let mut item = style_item(1, 9, LoroValue::String("old".into()));
        item.try_replace(&style_item(1, 8, LoroValue::String("ignored".into())));
        assert_eq!(item.value, LoroValue::String("old".into()));

        item.try_replace(&style_item(1, 10, LoroValue::String("peer-wins".into())));
        assert_eq!(item.value, LoroValue::String("peer-wins".into()));

        item.try_replace(&style_item(2, 0, LoroValue::String("lamport-wins".into())));
        assert_eq!(item.value, LoroValue::String("lamport-wins".into()));
    }

    #[test]
    fn style_meta_compose_keeps_latest_value_per_key_and_preserves_new_keys() {
        let mut left = StyleMeta::default();
        left.insert(key("bold"), style_item(2, 1, LoroValue::Bool(true)));
        left.insert(
            key("color"),
            style_item(1, 1, LoroValue::String("red".into())),
        );

        let mut right = StyleMeta::default();
        right.insert(key("bold"), style_item(1, 99, LoroValue::Bool(false)));
        right.insert(
            key("color"),
            style_item(3, 0, LoroValue::String("blue".into())),
        );
        right.insert(
            key("link"),
            style_item(1, 1, LoroValue::String("docs".into())),
        );

        left.compose(&right, (DeltaType::Retain, DeltaType::Retain));
        let values = left.to_map();
        assert_eq!(values.get("bold"), Some(&LoroValue::Bool(true)));
        assert_eq!(values.get("color"), Some(&LoroValue::String("blue".into())));
        assert_eq!(values.get("link"), Some(&LoroValue::String("docs".into())));
    }

    #[test]
    fn style_meta_value_views_distinguish_null_from_absent() {
        let mut meta = StyleMeta::default();
        assert!(meta.is_empty());
        assert_eq!(meta.to_option_map(), None);

        meta.insert(key("bold"), style_item(1, 1, LoroValue::Bool(true)));
        meta.insert(key("deleted"), style_item(1, 2, LoroValue::Null));

        assert!(meta.contains_key(&key("deleted")));
        assert_eq!(
            meta.to_map().get("deleted"),
            Some(&LoroValue::Null),
            "to_map keeps explicit null style values"
        );
        assert_eq!(
            meta.to_map_without_null_value().get("deleted"),
            None,
            "serialized style values omit null entries"
        );
        assert_eq!(
            meta.to_value(),
            LoroValue::Map(
                FxHashMap::from_iter([(String::from("bold"), LoroValue::Bool(true))]).into()
            )
        );

        let styles: FxHashMap<_, _> = meta.iter().map(|style| (style.0, style.1.data)).collect();
        assert_eq!(styles.get(&key("bold")), Some(&LoroValue::Bool(true)));
        assert_eq!(styles.get(&key("deleted")), Some(&LoroValue::Null));
    }

    #[test]
    fn text_meta_compose_and_json_roundtrip_are_map_like() {
        let mut left = TextMeta(FxHashMap::from_iter([(
            String::from("lang"),
            LoroValue::String("en".into()),
        )]));
        let right = TextMeta(FxHashMap::from_iter([
            (String::from("lang"), LoroValue::String("fr".into())),
            (String::from("author"), LoroValue::String("loro".into())),
        ]));

        left.compose(&right, (DeltaType::Retain, DeltaType::Retain));
        assert_eq!(left.0.get("lang"), Some(&LoroValue::String("fr".into())));
        assert_eq!(
            left.0.get("author"),
            Some(&LoroValue::String("loro".into()))
        );
        assert!(left.is_mergeable(&left.clone()));

        let json = left.to_json_value();
        let decoded = TextMeta::from_json(&json.to_string());
        assert_eq!(decoded.0, left.0);
    }
}
