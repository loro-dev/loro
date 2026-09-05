//! Streaming bulk JSON reads. Container identity comes from CRDT edges, never
//! from the shape of user values. The sparse sidecar indexes *all* JSON values.
use super::{
    deleted_root_container_value_is_cleared, get_meta_value, state_decode_error,
    visible_container_value_is_empty, DocState,
};
use crate::{container::idx::ContainerIdx, ContainerType, LoroValue};
use loro_common::{ContainerID, LoroResult};
use std::sync::atomic::Ordering;

/// Plain JSON and a sparse index of container positions in its parsed value.
#[derive(Debug)]
pub struct DeepValueJsonWithIds {
    pub json: String,
    pub cids: Vec<String>,
    /// Zero-based pre-order positions: count every JSON value, including the
    /// document object, scalars, plain objects/arrays, and binary array items.
    /// Object children follow JavaScript's `Object.keys` order.
    pub container_positions: Vec<u32>,
}

#[derive(Default)]
struct JsonWriter {
    bytes: Vec<u8>,
    cids: Vec<String>,
    positions: Vec<u32>,
    next: u32,
    with_ids: bool,
}

impl JsonWriter {
    fn count(&mut self, n: u32) -> LoroResult<()> {
        if !self.with_ids {
            return Ok(());
        }
        self.next = self.next.checked_add(n).ok_or_else(|| {
            state_decode_error("Bulk JSON has too many values for its container position index")
        })?;
        Ok(())
    }

    fn json(&mut self, value: &impl serde::Serialize) -> LoroResult<()> {
        serde_json::to_writer(&mut self.bytes, value)
            .map_err(|e| state_decode_error(format!("Failed to serialize deep value: {e}")))
    }

    fn finish(self) -> LoroResult<DeepValueJsonWithIds> {
        Ok(DeepValueJsonWithIds {
            json: String::from_utf8(self.bytes)
                .map_err(|e| state_decode_error(format!("Invalid bulk JSON UTF-8: {e}")))?,
            cids: self.cids,
            container_positions: self.positions,
        })
    }

    fn container(&mut self, state: &mut DocState, idx: ContainerIdx) -> LoroResult<()> {
        let id = state.arena.idx_to_id(idx).unwrap();
        let value = state
            .store
            .get_value_ephemeral(idx)
            .unwrap_or_else(|| idx.get_type().default_value());
        self.container_value(state, &id, value)
    }

    fn container_value(
        &mut self,
        state: &mut DocState,
        id: &ContainerID,
        mut value: LoroValue,
    ) -> LoroResult<()> {
        if self.with_ids {
            self.positions.push(self.next);
            self.cids.push(id.to_string());
        }
        // Match getDeepValueWithID: tree metadata is plain deep data, not
        // additional with-id nodes. Its JSON values still count as positions.
        if id.container_type() == ContainerType::Tree {
            if let LoroValue::List(list) = &mut value {
                get_meta_value(list.make_mut(), state);
            }
            return self.value(state, &value, None);
        }
        self.value(state, &value, Some(id))
    }

    fn child(&mut self, state: &mut DocState, value: &LoroValue, resolve: bool) -> LoroResult<()> {
        if resolve {
            if let LoroValue::Container(id) = value {
                let idx = state.arena.register_container(id);
                return self.container(state, idx);
            }
        }
        self.value(state, value, None)
    }

    fn value(
        &mut self,
        state: &mut DocState,
        value: &LoroValue,
        parent: Option<&ContainerID>,
    ) -> LoroResult<()> {
        self.count(1)?;
        match value {
            LoroValue::Map(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_unstable_by(|(a, _), (b, _)| json_key_cmp(a, b));
                self.bytes.push(b'{');
                for (i, (key, value)) in entries.into_iter().enumerate() {
                    if i > 0 {
                        self.bytes.push(b',');
                    }
                    self.json(key)?;
                    self.bytes.push(b':');
                    // Resolve compact mergeable markers only at actual map
                    // edges. Identical bytes in plain user data stay data.
                    let mergeable = parent.and_then(|id| {
                        loro_common::parse_mergeable_marker(id, key, value)
                            .map(|kind| ContainerID::new_mergeable(id, key, kind))
                    });
                    if let Some(id) = mergeable {
                        let idx = state.arena.register_container(&id);
                        self.container(state, idx)?;
                    } else {
                        self.child(state, value, parent.is_some())?;
                    }
                }
                self.bytes.push(b'}');
            }
            LoroValue::List(list) => {
                self.bytes.push(b'[');
                for (i, value) in list.iter().enumerate() {
                    if i > 0 {
                        self.bytes.push(b',');
                    }
                    self.child(state, value, parent.is_some())?;
                }
                self.bytes.push(b']');
            }
            LoroValue::Binary(bytes) => {
                self.count(
                    u32::try_from(bytes.len())
                        .map_err(|_| state_decode_error("Bulk JSON binary is too large"))?,
                )?;
                self.json(value)?;
            }
            _ => self.json(value)?,
        }
        Ok(())
    }
}

// ECMA-262 array-index property keys precede other keys in Object.keys, even
// after JSON.parse. 2^32-1, leading-zero spellings and negative keys are NOT
// array indices. Sorting other keys gives deterministic output on every target.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty()
        || key.len() > 10
        || (key.len() > 1 && key.starts_with('0'))
        || !key.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

fn json_key_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    match (array_index(a), array_index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    }
}

impl DocState {
    pub(super) fn write_deep_value_json(
        &mut self,
        with_ids: bool,
    ) -> LoroResult<DeepValueJsonWithIds> {
        let mut writer = JsonWriter {
            with_ids,
            ..Default::default()
        };
        writer.count(1)?; // The document object is value zero, not a container.
        writer.bytes.push(b'{');
        let mut roots: Vec<_> = self
            .preferred_root_containers()
            .into_iter()
            .map(|idx| (self.root_container_name(idx).unwrap(), idx))
            .collect();
        roots.sort_unstable_by(|(a, _), (b, _)| json_key_cmp(a, b));
        let hide_empty = self
            .config
            .hide_empty_root_containers
            .load(Ordering::Relaxed);
        let mut first = true;
        for (key, idx) in roots {
            let id = self.arena.idx_to_id(idx).unwrap();
            let value = self
                .store
                .get_value_ephemeral(idx)
                .unwrap_or_else(|| idx.get_type().default_value());
            if (hide_empty && visible_container_value_is_empty(idx.get_type(), &value))
                || (self.config.deleted_root_containers.lock().contains(&id)
                    && deleted_root_container_value_is_cleared(idx.get_type(), &value))
            {
                continue;
            }
            if !first {
                writer.bytes.push(b',');
            }
            first = false;
            writer.json(&key)?;
            writer.bytes.push(b':');
            writer.container_value(self, &id, value)?;
        }
        writer.bytes.push(b'}');
        writer.finish()
    }

    pub(super) fn write_container_deep_value_json(
        &mut self,
        idx: ContainerIdx,
        with_ids: bool,
    ) -> LoroResult<DeepValueJsonWithIds> {
        let mut writer = JsonWriter {
            with_ids,
            ..Default::default()
        };
        writer.container(self, idx)?;
        writer.finish()
    }
}
