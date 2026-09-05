//! Structured state traversal. Container identity comes from CRDT edges, never value shape.
//! A sink constructs the target representation without a whole-document intermediate tree.
use super::{deleted_root_container_value_is_cleared, visible_container_value_is_empty, DocState};
use crate::{container::idx::ContainerIdx, ContainerType, LoroValue};
use loro_common::{ContainerID, LoroError, LoroResult};
use std::sync::atomic::Ordering;
#[derive(Debug)]
pub enum Event<'a> {
    Object(usize),
    Array(usize),
    End,
    Key(&'a str),
    String(&'a str),
    Number(f64),
    Bool(bool),
    Null,
    Binary(&'a [u8]),
}
pub trait Sink {
    fn emit(&mut self, e: Event<'_>) -> LoroResult<()>;
    fn container(&mut self, id: &ContainerID) -> LoroResult<()>;
    fn container_end(&mut self) -> LoroResult<()> {
        self.emit(Event::End)
    }
    fn value_start(&mut self) -> LoroResult<()>;
    fn value_end(&mut self) -> LoroResult<()>;
}
impl DocState {
    pub fn read_state<S: Sink>(
        &mut self,
        s: &mut S,
        cid: Option<&ContainerID>,
        rich: bool,
        range: Option<(usize, usize)>,
    ) -> LoroResult<()> {
        if let Some(id) = cid {
            let idx = self.arena.register_container(id);
            if range.is_some()
                && !matches!(
                    idx.get_type(),
                    ContainerType::List | ContainerType::MovableList
                )
            {
                return Err(err("readState range requires a List or MovableList"));
            }
            return self.read_state_container(s, idx, rich, range, None, 0);
        }
        if range.is_some() {
            return Err(err("readState range requires a container"));
        }
        let roots = self.preferred_root_containers();
        let mut visible = Vec::new();
        for idx in roots {
            if matches!(idx.get_type(), ContainerType::Unknown(_)) {
                return Err(err("Unsupported container type"));
            }
            let id = self
                .arena
                .idx_to_id(idx)
                .ok_or_else(|| err("Missing container ID"))?;
            let hidden = self
                .config
                .hide_empty_root_containers
                .load(Ordering::Relaxed);
            let deleted = self.config.deleted_root_containers.lock().contains(&id);
            let v = if hidden || deleted {
                self.store
                    .try_get_value_ephemeral(idx)?
                    .or_else(|| Some(idx.get_type().default_value()))
            } else {
                None
            };
            if v.as_ref().is_some_and(|v| {
                (hidden && visible_container_value_is_empty(idx.get_type(), v))
                    || (deleted && deleted_root_container_value_is_cleared(idx.get_type(), v))
            }) {
                continue;
            }
            visible.push((
                self.root_container_name(idx)
                    .ok_or_else(|| err("Missing root name"))?,
                idx,
                v,
            ));
        }
        s.emit(Event::Object(visible.len()))?;
        for (key, idx, value) in visible {
            s.emit(Event::Key(&key))?;
            self.read_state_container(s, idx, rich, None, value, 0)?;
        }
        s.emit(Event::End)
    }
    fn read_state_container<S: Sink>(
        &mut self,
        s: &mut S,
        idx: ContainerIdx,
        rich: bool,
        range: Option<(usize, usize)>,
        value: Option<LoroValue>,
        depth: usize,
    ) -> LoroResult<()> {
        check_depth(depth)?;
        let id = self
            .arena
            .idx_to_id(idx)
            .ok_or_else(|| err("Missing container ID"))?;
        let kind = idx.get_type();
        if matches!(kind, ContainerType::Unknown(_)) {
            return Err(err("Unsupported container type"));
        }
        let v = if rich && kind == ContainerType::Text {
            self.store
                .get_or_create_mut(idx)
                .as_richtext_state_mut()
                .ok_or_else(|| err("Missing text state"))?
                .get_richtext_value()
        } else {
            match value {
                Some(value) => value,
                None => self
                    .store
                    .try_get_value_ephemeral(idx)?
                    .unwrap_or_else(|| kind.default_value()),
            }
        };
        s.container(&id)?;

        match (&v, kind) {
            (LoroValue::Map(m), ContainerType::Map) => {
                s.emit(Event::Object(m.len()))?;
                for (k, v) in m.iter() {
                    s.emit(Event::Key(k))?;
                    let merge = loro_common::parse_mergeable_marker(&id, k, v)
                        .map(|t| ContainerID::new_mergeable(&id, k, t));
                    if let Some(c) = merge {
                        let i = self.arena.register_container(&c);
                        self.read_state_container(s, i, rich, None, None, depth + 1)?;
                    } else {
                        self.read_state_edge(s, v, rich, depth + 1)?;
                    }
                }
                s.emit(Event::End)?;
            }
            (LoroValue::List(l), ContainerType::List | ContainerType::MovableList) => {
                let (start, end) = range.unwrap_or((0, l.len()));
                let start = start.min(l.len());
                let end = end.min(l.len()).max(start);
                s.emit(Event::Array(end - start))?;
                for v in &l[start..end] {
                    self.read_state_edge(s, v, rich, depth + 1)?;
                }
                s.emit(Event::End)?;
            }
            (_, ContainerType::Tree) => self.read_state_tree(s, &v, rich, depth + 1)?,
            _ => raw(s, &v, depth + 1)?,
        };
        s.container_end()
    }
    fn read_state_edge<S: Sink>(
        &mut self,
        s: &mut S,
        v: &LoroValue,
        rich: bool,
        depth: usize,
    ) -> LoroResult<()> {
        if let LoroValue::Container(cid) = v {
            let idx = self.arena.register_container(cid);
            return self.read_state_container(s, idx, rich, None, None, depth);
        }
        s.value_start()?;
        raw(s, v, depth + 1)?;
        s.value_end()
    }
    fn read_state_tree<S: Sink>(
        &mut self,
        s: &mut S,
        v: &LoroValue,
        rich: bool,
        depth: usize,
    ) -> LoroResult<()> {
        check_depth(depth)?;
        match v {
            LoroValue::List(l) => {
                s.emit(Event::Array(l.len()))?;
                for node in l.iter() {
                    let m = node.as_map().ok_or_else(|| err("Invalid tree node"))?;
                    s.emit(Event::Object(m.len()))?;
                    for (k, v) in m.iter() {
                        s.emit(Event::Key(k))?;
                        if k == "meta" {
                            self.read_state_edge(s, v, rich, depth + 1)?;
                        } else if k == "children" {
                            self.read_state_tree(s, v, rich, depth + 1)?;
                        } else {
                            raw(s, v, depth + 1)?;
                        }
                    }
                    s.emit(Event::End)?;
                }
                s.emit(Event::End)
            }
            _ => Err(err("Invalid tree value")),
        }
    }
}
pub fn err(s: &str) -> LoroError {
    LoroError::JsError(s.to_string().into_boxed_str())
}
fn raw<S: Sink>(s: &mut S, v: &LoroValue, depth: usize) -> LoroResult<()> {
    check_depth(depth)?;
    match v {
        LoroValue::Null => s.emit(Event::Null),
        LoroValue::Bool(b) => s.emit(Event::Bool(*b)),
        // Match the existing JavaScript value conversion, including i64 -> number.
        LoroValue::I64(n) => s.emit(Event::Number(*n as f64)),
        LoroValue::Double(n) => s.emit(Event::Number(*n)),
        LoroValue::Binary(v) => s.emit(Event::Binary(v)),
        LoroValue::Container(id) => s.emit(Event::String(&id.to_string())),
        LoroValue::String(v) => s.emit(Event::String(v)),
        LoroValue::List(l) => {
            s.emit(Event::Array(l.len()))?;
            for v in l.iter() {
                raw(s, v, depth + 1)?;
            }
            s.emit(Event::End)
        }
        LoroValue::Map(m) => {
            s.emit(Event::Object(m.len()))?;
            for (k, v) in m.iter() {
                s.emit(Event::Key(k))?;
                raw(s, v, depth + 1)?;
            }
            s.emit(Event::End)
        }
    }
}

fn check_depth(depth: usize) -> LoroResult<()> {
    if depth > 256 {
        Err(err("readState nesting exceeds 256 levels"))
    } else {
        Ok(())
    }
}
