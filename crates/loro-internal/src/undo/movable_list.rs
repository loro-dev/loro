//! Identity and gap anchors for MovableList undo. Public list deltas deliberately
//! carry values, so they cannot distinguish a moved scalar from a new copy.

use std::{collections::BTreeMap, ops::Bound};

use fractional_index::FractionalIndex;
use loro_common::{CompactIdLp, ContainerID, IdLp, LoroResult, LoroValue};
use loro_delta::DeltaRopeBuilder;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    event::{Diff, ListDeltaMeta},
    handler::{Handler, MovableListHandler, ValueOrHandler},
    state::IndexType,
    HandlerTrait, LoroDoc,
};

use super::DiffBatch;

pub(crate) type ElementRemap = FxHashMap<CompactIdLp, CompactIdLp>;

#[derive(Clone)]
struct Element {
    id: CompactIdLp,
    position: IdLp,
    value_id: IdLp,
    value: LoroValue,
}

pub(crate) struct Snapshot {
    alive: bool,
    elements: Vec<Element>,
}

impl Snapshot {
    pub(crate) fn capture(doc: &LoroDoc, cid: &ContainerID) -> Self {
        let mut state = doc.state.lock();
        let alive = state.get_reachable(cid);
        let elements = doc.arena.id_to_idx(cid).map_or_else(Vec::new, |idx| {
            state.with_state(idx, |s| {
                let list = s.as_movable_list_state().unwrap();
                list.iter_with_last_move_id_and_elem_id()
                    .map(|(pos, id, value)| Element {
                        id,
                        position: pos.idlp(),
                        value_id: list.elements()[&id].value_id,
                        value: value.clone(),
                    })
                    .collect()
            })
        });
        Self { alive, elements }
    }
}

pub(crate) struct Inverse {
    pub span_index: usize,
    pub cid: ContainerID,
    pub before: Snapshot,
    pub after: Snapshot,
}

enum Edit {
    Delete(CompactIdLp),
    Place {
        id: CompactIdLp,
        // Some only for a removal undo; moves must never recreate a deleted element.
        insert: Option<LoroValue>,
        left: Option<CompactIdLp>,
        right: Option<CompactIdLp>,
    },
    Set(CompactIdLp, LoroValue),
}

pub(crate) struct Plan {
    kind: PlanKind,
}

enum PlanKind {
    Native(Vec<Edit>),
    Recreated(Vec<Element>),
}

pub(crate) type Revivals = FxHashMap<ContainerID, (usize, Snapshot)>;

/// Keep the identity source of a full-state child revival. The positional
/// delta keeps the values but cannot supply these ids after cloning its parent.
pub(crate) fn capture_revivals(
    doc: &LoroDoc,
    diff: &DiffBatch,
    span_index: usize,
    revivals: &mut Revivals,
) {
    for (cid, diff) in diff.iter() {
        if cid.container_type() != loro_common::ContainerType::MovableList {
            continue;
        }
        let Diff::List(delta) = diff else {
            unreachable!()
        };
        if delta
            .iter()
            .all(|item| matches!(item, loro_delta::DeltaItem::Replace { delete: 0, .. }))
        {
            let snapshot = Snapshot::capture(doc, cid);
            let inserted: usize = delta
                .iter()
                .map(|item| match item {
                    loro_delta::DeltaItem::Replace { value, .. } => value.len(),
                    _ => unreachable!(),
                })
                .sum();
            if inserted == snapshot.elements.len() {
                revivals.insert(cid.clone(), (span_index, snapshot));
            }
        }
    }
}

fn remap<T: Clone + Eq + std::hash::Hash>(id: &T, map: &FxHashMap<T, T>) -> T {
    let mut id = id.clone();
    while let Some(next) = map.get(&id) {
        id = next.clone();
    }
    id
}

fn remap_value(value: &LoroValue, containers: &FxHashMap<ContainerID, ContainerID>) -> LoroValue {
    match value {
        LoroValue::Container(cid) => LoroValue::Container(remap(cid, containers)),
        value => value.clone(),
    }
}

// The ordered model avoids scanning/shifting the live list for each edit. Only
// snapshots and the final public delta are linear in the list's length.
struct Model {
    order: BTreeMap<FractionalIndex, Element>,
    keys: FxHashMap<CompactIdLp, FractionalIndex>,
}

impl Model {
    fn new(elements: &[Element]) -> Self {
        let keys = FractionalIndex::generate_n_evenly(None, None, elements.len()).unwrap();
        let order: BTreeMap<_, _> = keys.into_iter().zip(elements.iter().cloned()).collect();
        let keys = order.iter().map(|(key, e)| (e.id, key.clone())).collect();
        Self { order, keys }
    }

    fn remove(&mut self, id: CompactIdLp) -> Option<Element> {
        let key = self.keys.remove(&id)?;
        Some(self.order.remove(&key).expect("undo model element missing"))
    }

    fn place(&mut self, element: Element, left: Option<CompactIdLp>, right: Option<CompactIdLp>) {
        let (lower, upper) = if let Some(key) = left.and_then(|id| self.keys.get(&id)) {
            let next = self
                .order
                .range((Bound::Excluded(key), Bound::Unbounded))
                .next();
            (Some(key), next.map(|(key, _)| key))
        } else if let Some(key) = right.and_then(|id| self.keys.get(&id)) {
            let prev = self.order.range(..key).next_back();
            (prev.map(|(key, _)| key), Some(key))
        } else {
            (None, self.order.first_key_value().map(|(key, _)| key))
        };
        let key = FractionalIndex::new(lower, upper).unwrap();
        assert!(self.keys.insert(element.id, key.clone()).is_none());
        assert!(self.order.insert(key, element).is_none());
    }

    fn undo(
        &mut self,
        inverse: &Inverse,
        remapped: &ElementRemap,
        containers: &FxHashMap<ContainerID, ContainerID>,
        edits: &mut Vec<Edit>,
    ) {
        let before: FxHashMap<_, _> = inverse.before.elements.iter().map(|e| (e.id, e)).collect();
        let after: FxHashMap<_, _> = inverse.after.elements.iter().map(|e| (e.id, e)).collect();
        for e in &inverse.after.elements {
            if !before.contains_key(&e.id) {
                let id = remap(&e.id, remapped);
                if self.remove(id).is_some() {
                    edits.push(Edit::Delete(id));
                }
            }
        }

        let repositioned: FxHashSet<_> = inverse
            .before
            .elements
            .iter()
            .filter_map(|e| {
                let id = remap(&e.id, remapped);
                match after.get(&e.id) {
                    None if !self.keys.contains_key(&id) => Some(id),
                    Some(other) if e.position != other.position && self.keys.contains_key(&id) => {
                        Some(id)
                    }
                    _ => None,
                }
            })
            .collect();
        // A future move is not a stable right anchor. Already placed elements
        // can anchor the next edit on the left, including restored runs.
        let mut right = vec![None; inverse.before.elements.len()];
        let mut next = None;
        for (i, e) in inverse.before.elements.iter().enumerate().rev() {
            right[i] = next;
            let id = remap(&e.id, remapped);
            if !repositioned.contains(&id) && self.keys.contains_key(&id) {
                next = Some(id);
            }
        }
        let mut left = None;
        for (i, e) in inverse.before.elements.iter().enumerate() {
            let id = remap(&e.id, remapped);
            if repositioned.contains(&id) {
                let insert =
                    (!after.contains_key(&e.id)).then(|| remap_value(&e.value, containers));
                let element = self.remove(id).unwrap_or_else(|| Element {
                    id,
                    value: insert.clone().unwrap(),
                    ..e.clone()
                });
                self.place(element, left, right[i]);
                edits.push(Edit::Place {
                    id,
                    insert,
                    left,
                    right: right[i],
                });
            }
            if let Some(other) = after.get(&e.id) {
                if e.value_id != other.value_id {
                    if let Some(key) = self.keys.get(&id) {
                        let element = self.order.get_mut(key).unwrap();
                        element.value = remap_value(&e.value, containers);
                        element.value_id = e.value_id;
                        edits.push(Edit::Set(id, element.value.clone()));
                    }
                }
            }
            if self.keys.contains_key(&id) {
                left = Some(id);
            }
        }
    }
}

/// Replace only lists that keep the same container lifetime. Parent-driven
/// recreation still uses the existing full-child diff and container remapping.
pub(crate) fn prepare(
    doc: &LoroDoc,
    diff: &mut DiffBatch,
    inverses: Vec<Inverse>,
    revivals: Revivals,
    containers: &FxHashMap<ContainerID, ContainerID>,
    remapped: &ElementRemap,
) -> FxHashMap<ContainerID, Plan> {
    let mut grouped: FxHashMap<ContainerID, Vec<Inverse>> = FxHashMap::default();
    for inverse in inverses.into_iter().rev() {
        grouped
            .entry(inverse.cid.clone())
            .or_default()
            .push(inverse);
    }
    let mut plans = FxHashMap::default();
    for (cid, (span_index, source)) in revivals {
        let mut elements = source.elements;
        for e in &mut elements {
            e.id = remap(&e.id, remapped);
            e.value = remap_value(&e.value, containers);
        }
        let mut model = Model::new(&elements);
        if let Some(inverses) = grouped.get(&cid) {
            for inverse in inverses.iter().filter(|i| i.span_index < span_index) {
                model.undo(inverse, remapped, containers, &mut Vec::new());
            }
        }
        plans.insert(
            cid,
            Plan {
                kind: PlanKind::Recreated(model.order.into_values().collect()),
            },
        );
    }
    for (cid, inverses) in grouped {
        let current = Snapshot::capture(doc, &remap(&cid, containers));
        if !current.alive || inverses.iter().any(|i| !i.before.alive || !i.after.alive) {
            continue;
        }
        // A parent clone normally installs aliases from its full-state source.
        // If that source could not be aligned, retain the previous positional
        // path rather than treating every recreated element as remotely deleted.
        if remap(&cid, containers) != cid {
            let current_ids: FxHashSet<_> = current.elements.iter().map(|e| e.id).collect();
            if inverses
                .iter()
                .flat_map(|i| &i.after.elements)
                .any(|e| remap(&e.id, remapped) == e.id && !current_ids.contains(&e.id))
            {
                continue;
            }
        }
        let mut model = Model::new(&current.elements);
        let mut edits = Vec::new();
        for inverse in &inverses {
            model.undo(inverse, remapped, containers, &mut edits);
        }
        let target: Vec<_> = model.order.into_values().collect();
        // Trim unchanged ends. The middle is only for the existing unknown-
        // container preflight; applying the plan emits native identity edits.
        let equal = |a: &Element, b: &Element| a.id == b.id && a.value_id == b.value_id;
        let prefix = current
            .elements
            .iter()
            .zip(&target)
            .take_while(|(a, b)| equal(a, b))
            .count();
        let suffix = current.elements[prefix..]
            .iter()
            .rev()
            .zip(target[prefix..].iter().rev())
            .take_while(|(a, b)| equal(a, b))
            .count();
        let delta = DeltaRopeBuilder::new()
            .retain(prefix, ListDeltaMeta::default())
            .delete(current.elements.len() - prefix - suffix)
            .insert_many(
                target[prefix..target.len() - suffix]
                    .iter()
                    .map(|e| ValueOrHandler::Value(e.value.clone())),
                ListDeltaMeta::default(),
            );
        if !diff.cid_to_events.contains_key(&cid) {
            diff.order.push(cid.clone());
        }
        diff.cid_to_events
            .insert(cid.clone(), Diff::List(delta.build()));
        plans.insert(
            cid,
            Plan {
                kind: PlanKind::Native(edits),
            },
        );
    }
    plans
}

impl Plan {
    pub(crate) fn apply(
        self,
        list: &MovableListHandler,
        diff: Diff,
        containers: &mut FxHashMap<ContainerID, ContainerID>,
        remapped: &mut ElementRemap,
    ) -> LoroResult<()> {
        match self.kind {
            PlanKind::Recreated(target) => {
                Handler::MovableList(list.clone()).apply_diff(
                    diff,
                    containers,
                    &mut Default::default(),
                )?;
                let current = list.with_state(|s| {
                    Ok(s.as_movable_list_state()
                        .unwrap()
                        .iter_with_last_move_id_and_elem_id()
                        .map(|(_, id, value)| (id, value.clone()))
                        .collect::<Vec<_>>())
                })?;
                if current.len() == target.len()
                    && current
                        .iter()
                        .zip(&target)
                        .all(|((_, value), e)| *value == remap_value(&e.value, containers))
                {
                    for ((new_id, _), e) in current.into_iter().zip(target) {
                        let old_id = remap(&e.id, remapped);
                        if old_id != new_id {
                            remapped.insert(old_id, new_id);
                        }
                    }
                }
                Ok(())
            }
            PlanKind::Native(edits) => {
                Self::apply_native(edits, list, containers, remapped)
                    .expect("validated MovableList undo plan must apply");
                Ok(())
            }
        }
    }

    fn apply_native(
        edits: Vec<Edit>,
        list: &MovableListHandler,
        containers: &mut FxHashMap<ContainerID, ContainerID>,
        remapped: &mut ElementRemap,
    ) -> LoroResult<()> {
        let index = |id: CompactIdLp, remapped: &ElementRemap| {
            let id = remap(&id, remapped);
            list.with_state(|s| Ok(s.as_movable_list_state().unwrap().get_index_of_elem(id)))
                .unwrap()
        };
        for edit in edits {
            match edit {
                Edit::Delete(id) => {
                    list.delete(index(id, remapped).expect("undo element disappeared"), 1)?
                }
                Edit::Place {
                    id,
                    insert,
                    left,
                    right,
                } => {
                    let gap = if let Some(left) = left {
                        index(left, remapped).expect("undo left anchor disappeared") + 1
                    } else if let Some(right) = right {
                        index(right, remapped).expect("undo right anchor disappeared")
                    } else {
                        0
                    };
                    if let Some(value) = insert {
                        assert!(
                            index(id, remapped).is_none(),
                            "undo element already restored"
                        );
                        match remap_value(&value, containers) {
                            LoroValue::Container(cid) => {
                                let child = list.insert_container(
                                    gap,
                                    Handler::new_unattached(cid.container_type())?,
                                )?;
                                containers.insert(cid, child.id());
                            }
                            value => list.insert(gap, value)?,
                        }
                        let new_id = list.with_state(|s| {
                            Ok(s.as_movable_list_state()
                                .unwrap()
                                .get_elem_id_at(gap, IndexType::ForUser)
                                .unwrap())
                        })?;
                        remapped.insert(remap(&id, remapped), new_id);
                    } else {
                        let from = index(id, remapped).expect("undo move element disappeared");
                        list.mov(from, gap - usize::from(from < gap))?;
                    }
                }
                Edit::Set(id, value) => {
                    let index = index(id, remapped).expect("undo set element disappeared");
                    match remap_value(&value, containers) {
                        LoroValue::Container(cid) => {
                            if list.get(index) != Some(LoroValue::Container(cid.clone())) {
                                let child = list.set_container(
                                    index,
                                    Handler::new_unattached(cid.container_type())?,
                                )?;
                                containers.insert(cid, child.id());
                            }
                        }
                        value => list.set(index, value)?,
                    }
                }
            }
        }
        Ok(())
    }
}
