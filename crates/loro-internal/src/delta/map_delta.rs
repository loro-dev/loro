use std::{hash::Hash, sync::Weak};

use loro_common::IdLp;
use rustc_hash::FxHashMap;
use serde::{ser::SerializeStruct, Serialize};

use crate::{
    change::Lamport, handler::ValueOrHandler, id::PeerID, span::HasLamport, InternalString,
    LoroDocInner, LoroValue,
};

#[derive(Default, Debug, Clone, Serialize)]
pub struct MapDelta {
    /// If the value is none, it's a uncreate op that should remove the entry from the
    /// map container.
    pub updated: FxHashMap<InternalString, Option<MapValue>>,
}

#[derive(Debug, Clone)]
pub struct MapValue {
    pub value: Option<LoroValue>,
    pub lamp: Lamport,
    pub peer: PeerID,
}

impl Ord for MapValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamp
            .cmp(&other.lamp)
            .then_with(|| self.peer.cmp(&other.peer))
    }
}

impl PartialOrd for MapValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        self.lamp == other.lamp && self.peer == other.peer
    }
}

impl Eq for MapValue {}

impl MapValue {
    pub fn idlp(&self) -> IdLp {
        IdLp::new(self.peer, self.lamp)
    }
}

#[derive(Default, Debug, Clone)]
pub struct ResolvedMapDelta {
    pub updated: FxHashMap<InternalString, ResolvedMapValue>,
}

#[derive(Debug, Clone)]
pub struct ResolvedMapValue {
    pub value: Option<ValueOrHandler>,
    pub idlp: IdLp,
}

impl ResolvedMapValue {
    pub(crate) fn from_map_value(v: MapValue, doc: &Weak<LoroDocInner>) -> Self {
        let doc = &doc.upgrade().unwrap();
        ResolvedMapValue {
            idlp: IdLp::new(v.peer, v.lamp),
            value: v.value.map(|v| ValueOrHandler::from_value(v, doc)),
        }
    }

    /// This is used to indicate that the entry is unset. (caused by checkout to before the entry is created)
    pub fn new_unset() -> Self {
        ResolvedMapValue {
            idlp: IdLp::new(PeerID::default(), Lamport::MAX),
            value: None,
        }
    }
}

impl MapDelta {
    pub(crate) fn compose(mut self, x: MapDelta) -> MapDelta {
        for (k, v) in x.updated.into_iter() {
            if let Some(old) = self.updated.get_mut(&k) {
                if &v > old {
                    *old = v;
                }
            } else {
                self.updated.insert(k, v);
            }
        }
        self
    }

    #[inline]
    pub fn new() -> Self {
        MapDelta {
            updated: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn with_entry(mut self, key: InternalString, map_value: MapValue) -> Self {
        self.updated.insert(key, Some(map_value));
        self
    }
}

impl ResolvedMapDelta {
    pub(crate) fn compose(&self, x: ResolvedMapDelta) -> ResolvedMapDelta {
        let mut updated = self.updated.clone();
        for (k, v) in x.updated.into_iter() {
            if let Some(old) = updated.get_mut(&k) {
                if v.idlp > old.idlp {
                    *old = v;
                }
            } else {
                updated.insert(k, v);
            }
        }
        ResolvedMapDelta { updated }
    }

    #[inline]
    pub fn new() -> Self {
        ResolvedMapDelta {
            updated: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn with_entry(mut self, key: InternalString, map_value: ResolvedMapValue) -> Self {
        self.updated.insert(key, map_value);
        self
    }

    pub(crate) fn transform(&mut self, b: &ResolvedMapDelta, left_prior: bool) {
        for (k, _) in b.updated.iter() {
            if !left_prior {
                self.updated.remove(k);
            }
        }
    }
}

impl Hash for MapValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // value is not being hashed
        self.peer.hash(state);
        self.lamp.hash(state);
    }
}

impl HasLamport for MapValue {
    fn lamport(&self) -> Lamport {
        self.lamp
    }
}

impl Serialize for MapValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("MapValue", 2)?;
        s.serialize_field("value", &self.value)?;
        s.serialize_field("lamport", &self.lamp)?;
        s.serialize_field("id", &self.idlp())?;
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{Hash, Hasher};

    use rustc_hash::FxHasher;

    use crate::{handler::ValueOrHandler, LoroValue};

    use super::*;

    fn key(value: &str) -> InternalString {
        value.into()
    }

    fn map_value(lamp: Lamport, peer: PeerID, value: i64) -> MapValue {
        MapValue {
            value: Some(LoroValue::I64(value)),
            lamp,
            peer,
        }
    }

    fn resolved_value(lamp: Lamport, peer: PeerID, value: i64) -> ResolvedMapValue {
        ResolvedMapValue {
            value: Some(ValueOrHandler::Value(LoroValue::I64(value))),
            idlp: IdLp::new(peer, lamp),
        }
    }

    fn hash_map_value(value: &MapValue) -> u64 {
        let mut hasher = FxHasher::default();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn map_value_ordering_identity_and_hash_are_timestamp_based() {
        let lower_lamport = map_value(1, 9, 10);
        let higher_lamport = map_value(2, 1, 20);
        let lower_peer = map_value(2, 1, 30);
        let higher_peer = map_value(2, 2, 40);

        assert!(higher_lamport > lower_lamport);
        assert!(higher_peer > lower_peer);
        assert_eq!(higher_peer.idlp(), IdLp::new(2, 2));
        assert_eq!(higher_peer.lamport(), 2);

        let same_id_different_value = MapValue {
            value: Some(LoroValue::I64(999)),
            ..higher_peer.clone()
        };
        assert_eq!(higher_peer, same_id_different_value);
        assert_eq!(
            hash_map_value(&higher_peer),
            hash_map_value(&same_id_different_value)
        );
    }

    #[test]
    fn map_delta_compose_keeps_latest_timestamp_per_key() {
        let initial = MapDelta::new()
            .with_entry(key("same"), map_value(2, 1, 10))
            .with_entry(key("left-only"), map_value(1, 1, 11));

        let mut incoming = MapDelta::new()
            .with_entry(key("same"), map_value(2, 2, 20))
            .with_entry(key("right-only"), map_value(1, 1, 21));
        incoming.updated.insert(key("unset"), None);

        let composed = initial.compose(incoming);
        assert_eq!(
            composed
                .updated
                .get(&key("same"))
                .and_then(|value| value.as_ref())
                .and_then(|value| value.value.as_ref()),
            Some(&LoroValue::I64(20))
        );
        assert!(composed.updated.contains_key(&key("left-only")));
        assert!(composed.updated.contains_key(&key("right-only")));
        assert_eq!(composed.updated.get(&key("unset")), Some(&None));
    }

    #[test]
    fn map_delta_compose_does_not_let_older_entries_overwrite_newer_ones() {
        let initial = MapDelta::new().with_entry(key("same"), map_value(5, 1, 50));
        let incoming = MapDelta::new().with_entry(key("same"), map_value(4, 9, 40));

        let composed = initial.compose(incoming);
        let value = composed
            .updated
            .get(&key("same"))
            .and_then(|value| value.as_ref())
            .and_then(|value| value.value.as_ref());
        assert_eq!(value, Some(&LoroValue::I64(50)));
    }

    #[test]
    fn resolved_map_delta_compose_and_transform_apply_the_same_conflict_contract() {
        let left = ResolvedMapDelta::new()
            .with_entry(key("same"), resolved_value(1, 1, 10))
            .with_entry(key("left-only"), resolved_value(1, 1, 11));
        let right = ResolvedMapDelta::new()
            .with_entry(key("same"), resolved_value(1, 2, 20))
            .with_entry(key("right-only"), resolved_value(1, 1, 21));

        let composed = left.compose(right.clone());
        let value = composed.updated.get(&key("same")).unwrap();
        assert_eq!(value.idlp, IdLp::new(2, 1));
        assert_eq!(value.value.as_ref().unwrap().to_value(), LoroValue::I64(20));
        assert!(composed.updated.contains_key(&key("left-only")));
        assert!(composed.updated.contains_key(&key("right-only")));

        let mut left_prior = composed.clone();
        left_prior.transform(&right, true);
        assert!(left_prior.updated.contains_key(&key("same")));

        let mut right_prior = composed;
        right_prior.transform(&right, false);
        assert!(!right_prior.updated.contains_key(&key("same")));
        assert!(!right_prior.updated.contains_key(&key("right-only")));
        assert!(right_prior.updated.contains_key(&key("left-only")));
    }

    #[test]
    fn resolved_unset_uses_max_lamport_to_win_against_material_values() {
        let unset = ResolvedMapValue::new_unset();
        let value = resolved_value(Lamport::MAX - 1, PeerID::MAX, 1);

        let composed = ResolvedMapDelta::new()
            .with_entry(key("field"), value)
            .compose(ResolvedMapDelta::new().with_entry(key("field"), unset));
        let entry = composed.updated.get(&key("field")).unwrap();
        assert_eq!(entry.idlp, IdLp::new(PeerID::default(), Lamport::MAX));
        assert!(entry.value.is_none());
    }

    #[test]
    fn map_value_serialization_exposes_value_lamport_and_id() {
        let serialized = serde_json::to_value(map_value(7, 8, 42)).unwrap();
        assert_eq!(serialized["value"], serde_json::json!(42));
        assert_eq!(serialized["lamport"], serde_json::json!(7));
        assert_eq!(
            serialized["id"],
            serde_json::json!({ "peer": 8, "lamport": 7 })
        );
    }
}
