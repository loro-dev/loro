//! Deep values of containers, built without recursion.
//!
//! A container chain can be as deep as an untrusted peer makes it, through updates or
//! through a lazily loaded snapshot that import never inspects, so the walk keeps its
//! frames on the heap. See `context/deep-value-stack-safety.md`.

use loro_common::{ContainerID, ContainerType, LoroValue};

use super::{ContainerIdx, DocState};

/// Where a child container's deep value goes in its parent's value.
enum Slot {
    List(usize),
    Map(String),
    /// The `meta` of the n-th tree node in pre-order.
    TreeMeta(usize),
}

struct Frame {
    value: LoroValue,
    /// The parent's `cid` string when building `{ cid, value }` nodes.
    cid: Option<LoroValue>,
    with_id: bool,
    is_tree: bool,
    tree_metas: Vec<Option<LoroValue>>,
    /// Child containers still to resolve, the next one last.
    pending: Vec<(Slot, ContainerID)>,
}

enum Opened {
    Done(LoroValue),
    Frame(Frame),
}

/// Visit tree nodes in pre-order.
fn for_each_tree_node(nodes: &mut [LoroValue], mut f: impl FnMut(&mut LoroValue)) {
    let mut pending = nodes.iter_mut().rev().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        let map = node.as_map_mut().unwrap().make_mut();
        f(map.get_mut("meta").unwrap());
        let children = map.get_mut("children").unwrap().as_list_mut().unwrap();
        pending.extend(children.make_mut().iter_mut().rev());
    }
}

impl Frame {
    fn set(&mut self, slot: Slot, value: LoroValue) {
        match slot {
            Slot::List(i) => self.value.as_list_mut().unwrap().make_mut()[i] = value,
            Slot::Map(key) => {
                self.value
                    .as_map_mut()
                    .unwrap()
                    .make_mut()
                    .insert(key, value);
            }
            Slot::TreeMeta(i) => self.tree_metas[i] = Some(value),
        }
    }

    fn finish(mut self) -> LoroValue {
        if self.is_tree {
            let mut metas = std::mem::take(&mut self.tree_metas).into_iter();
            let nodes = self.value.as_list_mut().unwrap().make_mut();
            for_each_tree_node(nodes, |meta| *meta = metas.next().unwrap().unwrap());
        }
        match self.cid {
            Some(cid) if self.with_id => LoroValue::Map(
                (rustc_hash::FxHashMap::from_iter([
                    ("cid".to_string(), cid),
                    ("value".to_string(), self.value),
                ]))
                .into(),
            ),
            _ => self.value,
        }
    }
}

impl DocState {
    /// The deep value of `container`: child containers are replaced by their deep values,
    /// and with `with_id` every container becomes `{ cid, value }`. Tree node metas are
    /// always plain deep values. A container without state is its type's default value.
    pub(super) fn deep_value_of(
        &mut self,
        container: ContainerIdx,
        id: Option<ContainerID>,
        with_id: bool,
    ) -> LoroValue {
        let first = match self.open_deep_frame(container, id, with_id) {
            Opened::Done(v) => return v,
            Opened::Frame(f) => f,
        };
        let mut stack: Vec<(Frame, Option<Slot>)> = vec![(first, None)];
        loop {
            let (frame, _) = stack.last_mut().unwrap();
            if let Some((slot, cid)) = frame.pending.pop() {
                let child_with_id = frame.with_id && !matches!(slot, Slot::TreeMeta(_));
                let idx = self.arena.register_container(&cid);
                match self.open_deep_frame(idx, Some(cid), child_with_id) {
                    Opened::Done(v) => stack.last_mut().unwrap().0.set(slot, v),
                    Opened::Frame(f) => stack.push((f, Some(slot))),
                }
                continue;
            }

            let (frame, slot) = stack.pop().unwrap();
            let value = frame.finish();
            match (stack.last_mut(), slot) {
                (Some((parent, _)), Some(slot)) => parent.set(slot, value),
                _ => return value,
            }
        }
    }

    fn open_deep_frame(
        &mut self,
        container: ContainerIdx,
        id: Option<ContainerID>,
        with_id: bool,
    ) -> Opened {
        let Some(value) = self.store.get_value_ephemeral(container) else {
            return Opened::Done(container.get_type().default_value());
        };
        let id = id.or_else(|| self.arena.idx_to_id(container));
        // The `cid` is the container id's Display string, the same form that the
        // JS `container.id` getter returns (e.g. `cid:root-map:Map`).
        let cid = with_id.then(|| LoroValue::String(id.as_ref().unwrap().to_string().into()));
        let mut frame = Frame {
            value,
            cid,
            with_id,
            is_tree: false,
            tree_metas: Vec::new(),
            pending: Vec::new(),
        };
        match &mut frame.value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(list) if container.get_type() == ContainerType::Tree => {
                // Each tree node has an associated map container for its metadata; its
                // deep value goes into the node's `meta` field.
                frame.is_tree = true;
                let mut metas = Vec::new();
                for_each_tree_node(list.make_mut(), |meta| {
                    metas.push(meta.as_container().unwrap().clone());
                });
                frame.tree_metas = vec![None; metas.len()];
                frame.pending = metas
                    .into_iter()
                    .enumerate()
                    .map(|(i, cid)| (Slot::TreeMeta(i), cid))
                    .collect();
            }
            LoroValue::List(list) => {
                frame.pending = list
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| Some((Slot::List(i), v.as_container()?.clone())))
                    .collect();
            }
            LoroValue::Map(map) => {
                frame.pending = map
                    .iter()
                    .filter_map(|(k, v)| Some((Slot::Map(k.clone()), v.as_container()?.clone())))
                    .collect();
                // A map's mergeable children are encoded as compact binary markers in the map's
                // own value table (loro-dev/loro#759), so they are already present in `map`.
                // Derive the active children straight from that value — re-fetching the
                // `MapState` would force the snapshot-backed container to decode and break the
                // lazy-value invariant for roots. They are resolved after the plain children,
                // so a marker's child wins its logical key.
                if let Some(parent_id) = &id {
                    let mergeable = self.mergeable_children_from_value(parent_id, map);
                    frame.pending.extend(
                        mergeable
                            .into_iter()
                            .map(|(key, cid)| (Slot::Map(key.to_string()), cid)),
                    );
                }
            }
            _ => {}
        }

        if frame.pending.is_empty() {
            return Opened::Done(frame.finish());
        }
        frame.pending.reverse();
        Opened::Frame(frame)
    }
}
