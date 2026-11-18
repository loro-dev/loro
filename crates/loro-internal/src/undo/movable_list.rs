//! Identity and gap anchors for MovableList undo. Public list deltas deliberately
//! carry values, so they cannot distinguish a moved scalar from a new copy.

use std::{collections::BTreeMap, ops::Bound};

use fractional_index::FractionalIndex;
use loro_common::{CompactIdLp, ContainerID, IdLp, IdSpan, LoroError, LoroResult, LoroValue, ID};
use loro_delta::DeltaRopeBuilder;
use rle::HasLength;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    container::list::list_op::InnerListOp,
    event::{Diff, ListDeltaMeta},
    handler::{Handler, MovableListHandler, ValueOrHandler},
    op::InnerContent,
    state::{IndexType, MovableListState},
    txn::Transaction,
    version::Frontiers,
    HandlerTrait, LoroDoc,
};

use super::DiffBatch;

pub(crate) type ElementRemap = FxHashMap<CompactIdLp, CompactIdLp>;

#[derive(Clone, Debug)]
struct Element {
    index: usize,
    id: CompactIdLp,
    position: IdLp,
    value_id: IdLp,
    value: LoroValue,
}

pub(crate) struct Snapshot {
    alive: bool,
    elements: Vec<Element>,
    targets: FxHashSet<CompactIdLp>,
    full: bool,
    searches: Vec<AnchorSearch>,
}

#[derive(Default)]
pub(crate) struct Scope {
    ids: FxHashSet<CompactIdLp>,
    // Only needed for deletes referring to positions trimmed by a shallow root.
    full: bool,
}

pub(crate) type Scopes = FxHashMap<ContainerID, Scope>;

/// Insert/move/set name elements directly. A deletion names list positions;
/// resolve each position's insertion or move to its element, without walking
/// the live list. Trimmed positions retain the old full-state fallback.
pub(crate) fn scopes(
    doc: &LoroDoc,
    spans: &[(IdSpan, Frontiers)],
) -> LoroResult<(Vec<Scopes>, Scopes)> {
    let oplog = doc.oplog.lock();
    let mut all: Scopes = Default::default();
    let scopes = spans
        .iter()
        .map(|(span, _)| {
            let mut scopes: Scopes = Default::default();
            for op in oplog.iter_ops(*span) {
                if op.container().get_type() != loro_common::ContainerType::MovableList {
                    continue;
                }
                let cid = doc.arena.get_container_id(op.container()).ok_or_else(|| {
                    LoroError::NotFoundError("MovableList undo history container is missing".into())
                })?;
                let scope = scopes.entry(cid).or_default();
                match &op.op().content {
                    InnerContent::List(InnerListOp::Move { elem_id, .. }) => {
                        scope.ids.insert(elem_id.compact());
                    }
                    InnerContent::List(InnerListOp::Set(set)) => {
                        scope.ids.insert(set.elem_id.compact());
                    }
                    InnerContent::List(InnerListOp::Insert { .. }) => {
                        for i in 0..op.atom_len() {
                            scope
                                .ids
                                .insert(IdLp::new(op.peer, op.idlp().lamport + i as u32).compact());
                        }
                    }
                    InnerContent::List(InnerListOp::Delete(deletion)) => {
                        for i in 0..deletion.atom_len() {
                            let id = ID::new(
                                deletion.id_start.peer,
                                deletion.id_start.counter + i as i32,
                            );
                            let Some(position) = oplog.get_op_that_includes(id) else {
                                scope.full = true;
                                continue;
                            };
                            let element = match &position.content {
                                InnerContent::List(InnerListOp::Move { elem_id, .. }) => *elem_id,
                                _ => {
                                    let Some(lamport) = oplog.get_lamport_at(id) else {
                                        scope.full = true;
                                        continue;
                                    };
                                    IdLp::new(id.peer, lamport)
                                }
                            };
                            scope.ids.insert(element.compact());
                        }
                    }
                    _ => {}
                }
            }
            for (cid, scope) in &scopes {
                let union = all.entry(cid.clone()).or_default();
                union.ids.extend(scope.ids.iter().copied());
                union.full |= scope.full;
            }
            Ok(scopes)
        })
        .collect::<LoroResult<Vec<_>>>()?;
    Ok((scopes, all))
}

#[derive(Clone, Debug)]
struct AnchorSearch {
    next: isize,
    direction: isize,
    width: usize,
    exhausted: bool,
    done: bool,
    candidates: Vec<Element>,
}

impl Element {
    fn from_state(index: usize, id: CompactIdLp, e: &crate::state::MovableListElement) -> Self {
        Self {
            index,
            id,
            position: e.pos,
            value_id: e.value_id,
            value: e.value.clone(),
        }
    }
}

fn element_at(list: &MovableListState, index: usize) -> Option<Element> {
    let (id, e) = list.get_elem_at_given_pos(index, IndexType::ForUser)?;
    #[cfg(test)]
    CAPTURE_STATS.with(|stats| {
        let (n, model) = stats.get();
        stats.set((n + 1, model));
    });
    Some(Element::from_state(index, id, e))
}

fn element_by_id(list: &MovableListState, id: CompactIdLp) -> Option<Element> {
    let e = element_at(list, list.get_index_of_elem(id)?)?;
    (e.id == id).then_some(e)
}

impl AnchorSearch {
    fn extend(&mut self, list: &MovableListState, scope: &Scope) {
        let mut found = 0;
        while self.next >= 0 && self.next < list.len() as isize {
            let index = self.next as usize;
            self.next += self.direction;
            let Some(e) = element_at(list, index) else {
                continue;
            };
            if scope.ids.contains(&e.id) {
                continue;
            }
            self.candidates.push(e);
            found += 1;
            if found >= self.width {
                break;
            }
        }
        self.exhausted = self.next < 0 || self.next >= list.len() as isize;
        self.width *= 2;
    }
}

impl Snapshot {
    pub(crate) fn capture(doc: &LoroDoc, cid: &ContainerID) -> Self {
        let mut state = doc.state.lock();
        let alive = state.get_reachable(cid);
        let elements = doc.arena.id_to_idx(cid).map_or_else(Vec::new, |idx| {
            state.with_state(idx, |s| {
                let list = s.as_movable_list_state().unwrap();
                list.iter_with_last_move_id_and_elem_id()
                    .enumerate()
                    .map(|(index, (pos, id, value))| Element {
                        index,
                        id,
                        position: pos.idlp(),
                        value_id: list.elements()[&id].value_id,
                        value: value.clone(),
                    })
                    .collect()
            })
        });
        #[cfg(test)]
        CAPTURE_STATS.with(|stats| {
            let (n, model) = stats.get();
            stats.set((n + elements.len(), model));
        });
        let targets = elements.iter().map(|e| e.id).collect();
        Self {
            alive,
            elements,
            targets,
            full: true,
            searches: Vec::new(),
        }
    }

    pub(crate) fn capture_span(
        doc: &LoroDoc,
        cid: &ContainerID,
        scope: &Scope,
        all: &Scope,
        before: bool,
    ) -> Self {
        if scope.full || all.full {
            return Self::capture(doc, cid);
        }
        let mut state = doc.state.lock();
        let alive = state.get_reachable(cid);
        let (elements, searches) = doc
            .arena
            .id_to_idx(cid)
            .map_or_else(Default::default, |idx| {
                state.with_state(idx, |s| {
                    let list = s.as_movable_list_state().unwrap();
                    let ids = if before { &all.ids } else { &scope.ids };
                    let elements: Vec<_> = ids
                        .iter()
                        .filter_map(|id| element_by_id(list, *id))
                        .collect();
                    let mut searches = Vec::new();
                    if before {
                        for e in elements.iter().filter(|e| scope.ids.contains(&e.id)) {
                            for direction in [-1, 1] {
                                let mut search = AnchorSearch {
                                    next: e.index as isize + direction,
                                    direction,
                                    width: 1,
                                    exhausted: false,
                                    done: false,
                                    candidates: Vec::new(),
                                };
                                search.extend(list, all);
                                searches.push(search);
                            }
                        }
                    }
                    (elements, searches)
                })
            });
        let mut result = Self {
            alive,
            elements,
            searches,
            targets: scope.ids.clone(),
            full: false,
        };
        result.elements.sort_by_key(|e| e.index);
        result
    }
}

/// A local movable-list op, in user indexes, as recorded for undo
/// ([`RecordedInverse`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum ListEdit {
    Insert { pos: usize, len: usize },
    Delete { start: usize, len: usize },
    Move { from: usize, to: usize },
    Set { index: usize },
}

/// A range deletion longer than this is not recorded (undo checks out): every
/// deleted element gets its own anchor searches.
const MAX_RECORDED_DELETE: usize = 64;
/// Anchor candidates recorded on each side; [`complete_anchors`] searches
/// history only when none of them survives.
const RECORDED_CANDIDATES: usize = 2;

/// What undo of one local movable-list op reads from the versions around it,
/// recorded while the op is applied instead of checked out later
/// (context/undo-log.md): the element snapshots of [`Snapshot::capture_span`]
/// before and after, and the first anchor candidates of each search.
#[derive(Debug, Clone)]
pub(crate) struct RecordedInverse {
    pub(crate) cid: ContainerID,
    /// The op (for merged inserts or deletes, the first one's kind).
    edit: ListEdit,
    before_alive: bool,
    after_alive: bool,
    before: Vec<Element>,
    after: Vec<Element>,
    targets: FxHashSet<CompactIdLp>,
    /// The anchor searches left and right of the targets (one contiguous
    /// range, so shared by all of them); `None` without targets before.
    sides: Option<[AnchorSearch; 2]>,
}

impl RecordedInverse {
    /// Reads the state before `edit`; `None` if it is not recorded.
    pub(crate) fn begin(
        list: &MovableListState,
        cid: ContainerID,
        alive: bool,
        edit: ListEdit,
    ) -> Option<Self> {
        let mut before = Vec::new();
        let (start, len) = match edit {
            ListEdit::Insert { .. } => (0, 0),
            ListEdit::Delete { start, len } => {
                if len > MAX_RECORDED_DELETE {
                    return None;
                }
                (start, len)
            }
            ListEdit::Move { from: index, .. } | ListEdit::Set { index } => (index, 1),
        };
        if len > 0 {
            list.walk_user(start, 1, |index, id, e| {
                before.push(Element::from_state(index, id, e));
                before.len() < len
            });
            if before.len() != len {
                return None;
            }
        }
        let targets: FxHashSet<_> = before.iter().map(|e| e.id).collect();
        let mut sides = None;
        if !before.is_empty() {
            // The targets are contiguous (one element, or one deleted range),
            // so every target's nearest non-target neighbours are the range's.
            let first = before.first().unwrap().index as isize;
            let last = before.last().unwrap().index as isize;
            let len = list.len() as isize;
            sides = Some([(first - 1, -1), (last + 1, 1)].map(|(start, direction)| {
                // As `AnchorSearch::extend` with a width of
                // `RECORDED_CANDIDATES`, walking instead of one query per index.
                let mut candidates = Vec::new();
                let mut next = start;
                if start >= 0 && start < len {
                    list.walk_user(start as usize, direction, |index, id, e| {
                        next = index as isize + direction;
                        if !targets.contains(&id) {
                            candidates.push(Element::from_state(index, id, e));
                        }
                        candidates.len() < RECORDED_CANDIDATES
                    });
                }
                AnchorSearch {
                    next,
                    direction,
                    width: RECORDED_CANDIDATES * 2,
                    exhausted: next < 0 || next >= len,
                    done: false,
                    candidates,
                }
            }));
        }
        Some(Self {
            cid,
            edit,
            before_alive: alive,
            after_alive: alive,
            before,
            after: Vec::new(),
            targets,
            sides,
        })
    }

    /// Reads the state after the edit. The inserted elements are read later,
    /// where they end up ([`Self::refresh_after`]); here only their ids.
    pub(crate) fn finish(&mut self, list: &MovableListState) -> Option<()> {
        match self.edit {
            ListEdit::Insert { pos, len } => {
                let mut n = 0;
                list.walk_user(pos, 1, |_, id, _| {
                    self.targets.insert(id);
                    n += 1;
                    n < len
                });
                (n == len).then_some(())?;
            }
            ListEdit::Delete { .. } => {}
            ListEdit::Set { index } | ListEdit::Move { to: index, .. } => {
                let e = element_at(list, index)?;
                (e.id == self.before[0].id).then_some(())?;
                self.after.push(e);
            }
        }
        Some(())
    }

    /// The positional list diff a checkout reports for undoing this record (the
    /// event-level part of the inverse), built from the snapshots on use: it is
    /// rarely needed, and building it for every op cost more than recording.
    /// `None` for a list the span created (a checkout reports no diff for it).
    pub(crate) fn positional_diff(&self, doc: &LoroDoc) -> Option<Diff> {
        if !self.before_alive {
            return None;
        }
        let value = |v: &LoroValue| ValueOrHandler::from_value(v.clone(), &doc.inner);
        let delta = match self.edit {
            ListEdit::Insert { .. } => {
                // Delete the inserted elements that remain, by their final index.
                let mut b = DeltaRopeBuilder::new();
                let mut cursor = 0;
                for e in &self.after {
                    b = b.retain(e.index - cursor, Default::default()).delete(1);
                    cursor = e.index + 1;
                }
                b.build()
            }
            ListEdit::Delete { .. } => DeltaRopeBuilder::new()
                .retain(self.before[0].index, Default::default())
                .insert_many(
                    self.before.iter().map(|e| value(&e.value)),
                    Default::default(),
                )
                .build(),
            ListEdit::Move { .. } => {
                let (from, to) = (self.before[0].index, self.after[0].index);
                let v = loro_delta::array_vec::ArrayVec::from([value(&self.after[0].value)]);
                let moved = ListDeltaMeta { from_move: true };
                if from < to {
                    DeltaRopeBuilder::new()
                        .retain(from, Default::default())
                        .insert(v, moved)
                        .retain(to - from, Default::default())
                        .delete(1)
                        .build()
                } else {
                    DeltaRopeBuilder::new()
                        .retain(to, Default::default())
                        .delete(1)
                        .retain(from - to, Default::default())
                        .insert(v, moved)
                        .build()
                }
            }
            ListEdit::Set { .. } => {
                let (old, new) = (&self.before[0], &self.after[0]);
                if old.value == new.value {
                    return None;
                }
                DeltaRopeBuilder::new()
                    .retain(new.index, Default::default())
                    .delete(1)
                    .insert(
                        loro_delta::array_vec::ArrayVec::from([value(&old.value)]),
                        Default::default(),
                    )
                    .build()
            }
        };
        Some(Diff::List(delta))
    }

    /// A list created by the span's own ops did not exist at its deps.
    pub(crate) fn mark_created_in(&mut self, span: IdSpan) {
        if super::created_in(&self.cid, span) {
            self.before_alive = false;
        }
    }

    /// Only inserts: nothing existed before, so there is nothing to anchor.
    pub(crate) fn is_insert_only(&self) -> bool {
        self.before.is_empty()
    }

    /// Only deletes (of one contiguous range each).
    pub(crate) fn is_delete_only(&self) -> bool {
        !self.before.is_empty() && self.after.is_empty() && self.targets.len() == self.before.len()
    }

    /// Joins the pieces of one deletion that the list split into several ops
    /// (dead list items between them), so the record describes the whole range
    /// as it was before the first piece. `false` (undo checks out) unless the
    /// pieces form one contiguous range.
    ///
    /// Pieces deleted right to left were recorded with their original indices.
    /// Pieces deleted left to right start at the same index, after the earlier
    /// pieces left: their indices, and those of their right anchor candidates,
    /// shift back by the length already deleted.
    pub(crate) fn merge_deletes(&mut self, other: &RecordedInverse) -> bool {
        let start = self.before.first().unwrap().index;
        let (left, right, before) = if other.before.last().unwrap().index + 1 == start {
            // `other` is to the left: it keeps its left search; the old range
            // keeps its right one.
            let mut before = other.before.clone();
            before.extend(self.before.iter().cloned());
            (
                other.sides.as_ref().unwrap()[0].clone(),
                self.sides.as_ref().unwrap()[1].clone(),
                before,
            )
        } else if other.before.first().unwrap().index == start {
            let shift = self.before.len();
            let mut before = self.before.clone();
            before.extend(other.before.iter().cloned().map(|mut e| {
                e.index += shift;
                e
            }));
            let mut right = other.sides.as_ref().unwrap()[1].clone();
            right.next += shift as isize;
            for e in &mut right.candidates {
                e.index += shift;
            }
            (self.sides.as_ref().unwrap()[0].clone(), right, before)
        } else {
            return false;
        };
        self.before = before;
        self.targets.extend(other.targets.iter().copied());
        self.sides = Some([left, right]);
        self.after_alive = other.after_alive;
        true
    }

    /// Adds `other`'s inserted elements, for several insert-only ops on one list.
    pub(crate) fn merge_inserts(&mut self, other: &RecordedInverse) {
        debug_assert!(self.is_insert_only() && other.is_insert_only());
        self.targets.extend(other.targets.iter().copied());
        self.after_alive = other.after_alive;
    }

    /// Reads the inserted elements that remain, where they are now: a later
    /// insert may have shifted an earlier one.
    pub(crate) fn refresh_after(&mut self, list: &MovableListState) {
        self.after = self
            .targets
            .iter()
            .filter_map(|id| element_by_id(list, *id))
            .collect();
        self.after.sort_by_key(|e| e.index);
    }

    /// Everything the record keeps, the elements' values included.
    pub(crate) fn estimated_bytes(&self) -> usize {
        let elements = self.before.iter().chain(&self.after).chain(
            self.sides
                .iter()
                .flatten()
                .flat_map(|s| s.candidates.iter()),
        );
        let element_bytes: usize = elements
            .map(|e| {
                std::mem::size_of::<Element>()
                    + e.value
                        .estimated_heap_bytes()
                        .saturating_sub(std::mem::size_of::<LoroValue>())
            })
            .sum();
        std::mem::size_of::<Self>()
            + crate::event::container_id_estimated_bytes(&self.cid)
            + element_bytes
            + self.targets.capacity() * (std::mem::size_of::<CompactIdLp>() + 1)
    }

    /// The [`Inverse`] a checkout at `deps` and at the span's last op would
    /// have produced.
    pub(crate) fn to_inverse(&self, deps: &Frontiers) -> Inverse {
        Inverse {
            span_index: 0,
            cid: self.cid.clone(),
            before: Snapshot {
                alive: self.before_alive,
                elements: self.before.clone(),
                targets: self.targets.clone(),
                full: false,
                // One pair per target, as a checkout's capture gives.
                searches: self
                    .sides
                    .iter()
                    .flat_map(|sides| self.before.iter().flat_map(move |_| sides.iter().cloned()))
                    .collect(),
            },
            after: Snapshot {
                alive: self.after_alive,
                elements: self.after.clone(),
                targets: self.targets.clone(),
                full: false,
                searches: Vec::new(),
            },
            before_frontiers: deps.clone(),
        }
    }
}

/// The [`Scopes`] of a span whose movable-list ops are all recorded: the
/// records name the same elements that [`scopes`] finds by reading the ops
/// (and, for deletes, the history of each deleted element), so a recorded
/// undo reads no history.
pub(crate) fn scopes_from_records(records: &[RecordedInverse]) -> Scopes {
    records
        .iter()
        .map(|r| {
            (
                r.cid.clone(),
                Scope {
                    ids: r.targets.clone(),
                    full: false,
                },
            )
        })
        .collect()
}

/// Whether `records` cover exactly the lists and elements the span's ops name
/// (`scopes`); checked in the undo log's verification mode.
pub(crate) fn records_match_scopes(records: &[RecordedInverse], scopes: &Scopes) -> bool {
    records.len() == scopes.len()
        && records.iter().all(|r| {
            scopes
                .get(&r.cid)
                .is_some_and(|scope| !scope.full && scope.ids == r.targets)
        })
}

/// Panics unless the two inverses (after [`complete_anchors`]) hold the same
/// data. Used by the undo log's verification mode.
///
/// On a shallow document value ids are not compared: a checkout rebuilds them
/// from the retained history, and a set trimmed before the shallow root leaves
/// it with another op's id, while the recorded snapshot reads the live state.
/// The recorded one is the state's; with the checkout's, undo issued a set that
/// changed nothing but the value id.
pub(crate) fn assert_same_inverses(from_checkout: &[Inverse], recorded: &[Inverse], shallow: bool) {
    let canonical = |inverses: &[Inverse]| -> Vec<String> {
        let mut out: Vec<String> = inverses
            .iter()
            .map(|i| {
                let elems = |s: &Snapshot| {
                    let mut v: Vec<_> = s
                        .elements
                        .iter()
                        .map(|e| {
                            let value_id = (!shallow).then_some(e.value_id);
                            format!(
                                "{}:{:?}:{:?}:{:?}:{:?}",
                                e.index, e.id, e.position, value_id, e.value
                            )
                        })
                        .collect();
                    v.sort();
                    v
                };
                let mut targets: Vec<_> =
                    i.before.targets.iter().map(|t| format!("{t:?}")).collect();
                targets.sort();
                format!(
                    "{:?} alive {}/{} targets {:?} before {:?} after {:?}",
                    i.cid,
                    i.before.alive,
                    i.after.alive,
                    targets,
                    elems(&i.before),
                    elems(&i.after)
                )
            })
            .collect();
        out.sort();
        out
    };
    assert_eq!(
        canonical(from_checkout),
        canonical(recorded),
        "the recorded movable-list inverse differs from the checkout's (left: checkout, right: recorded)"
    );
}

pub(crate) struct Inverse {
    pub span_index: usize,
    pub cid: ContainerID,
    pub before: Snapshot,
    pub after: Snapshot,
    pub before_frontiers: Frontiers,
}

/// Usually each immediate neighbour survives, so no additional checkout is
/// needed. If remote deletes removed it, expand just that search geometrically
/// in the original view until the nearest surviving neighbour is found. Never
/// copy a live-id table or the unrelated values of a large list.
pub(crate) fn complete_anchors(
    doc: &LoroDoc,
    inverses: &mut [Inverse],
    latest: &Frontiers,
    scopes: &Scopes,
    remapped: &ElementRemap,
    containers: &FxHashMap<ContainerID, ContainerID>,
) -> LoroResult<()> {
    for inverse in inverses {
        loop {
            {
                let mut state = doc.state.lock();
                if let Some(idx) = doc.arena.id_to_idx(&remap(&inverse.cid, containers)) {
                    state.with_state(idx, |s| {
                        let list = s.as_movable_list_state().unwrap();
                        for search in inverse.before.searches.iter_mut().filter(|s| !s.done) {
                            if let Some(e) = search
                                .candidates
                                .iter()
                                .find(|e| element_by_id(list, remap(&e.id, remapped)).is_some())
                            {
                                inverse.before.elements.push(e.clone());
                                search.done = true;
                            } else if search.exhausted {
                                search.done = true;
                            }
                        }
                    });
                }
            }
            if inverse.before.searches.iter().all(|s| s.done) {
                break;
            }
            doc._checkout_without_emitting(&inverse.before_frontiers, false, false)?;
            {
                let mut state = doc.state.lock();
                if let Some(idx) = doc.arena.id_to_idx(&inverse.cid) {
                    state.with_state(idx, |s| {
                        let list = s.as_movable_list_state().unwrap();
                        for search in inverse.before.searches.iter_mut().filter(|s| !s.done) {
                            search.extend(list, &scopes[&inverse.cid]);
                        }
                    });
                }
            }
            doc._checkout_without_emitting(latest, false, false)?;
        }
        inverse.before.elements.sort_by_key(|e| e.index);
        inverse.before.elements.dedup_by_key(|e| e.id);
        inverse.before.searches.clear();
    }
    doc.set_detached(false);
    Ok(())
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
    Native {
        edits: Vec<Edit>,
        preflight: NativePreflight,
    },
    Recreated(Vec<Element>),
}

/// The batch preflight needs actual child lifetimes, not a full value delta
/// of the sparse model. Native moves never create a child; only restore/set
/// can do so. This metadata is private to identity undo.
#[derive(Default)]
pub(crate) struct NativePreflight {
    pub creates: FxHashSet<ContainerID>,
    pub removes: FxHashSet<ContainerID>,
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
            let inserted: usize = delta
                .iter()
                .map(|item| match item {
                    loro_delta::DeltaItem::Replace { value, .. } => value.len(),
                    _ => unreachable!(),
                })
                .sum();
            // An insert-only delta can also restore just the first element
            // of a large live list. Check its length before taking a full
            // child-revival snapshot, not after cloning unrelated values.
            let matches_full_state = {
                let mut state = doc.state.lock();
                doc.arena.id_to_idx(cid).is_some_and(|idx| {
                    state.with_state(idx, |s| {
                        s.as_movable_list_state().unwrap().len() == inserted
                    })
                })
            };
            if matches_full_state {
                revivals.insert(cid.clone(), (span_index, Snapshot::capture(doc, cid)));
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

// The ordered model contains only affected elements and their anchors. Relative
// keys avoid shifting the rest of the list while planning each identity edit.
struct Model {
    order: BTreeMap<FractionalIndex, Element>,
    keys: FxHashMap<CompactIdLp, FractionalIndex>,
}

impl Model {
    fn new(elements: &[Element]) -> Self {
        #[cfg(test)]
        CAPTURE_STATS.with(|stats| {
            let (n, model) = stats.get();
            stats.set((n, model.max(elements.len())));
        });
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
        preflight: &mut NativePreflight,
    ) {
        let before: FxHashMap<_, _> = inverse.before.elements.iter().map(|e| (e.id, e)).collect();
        let after: FxHashMap<_, _> = inverse.after.elements.iter().map(|e| (e.id, e)).collect();
        for e in &inverse.after.elements {
            if inverse.after.targets.contains(&e.id) && !before.contains_key(&e.id) {
                let id = remap(&e.id, remapped);
                if let Some(removed) = self.remove(id) {
                    if let LoroValue::Container(cid) = removed.value {
                        preflight.removes.insert(cid);
                    }
                    edits.push(Edit::Delete(id));
                }
            }
        }

        let repositioned: FxHashSet<_> = inverse
            .before
            .elements
            .iter()
            .filter(|e| inverse.before.targets.contains(&e.id))
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
            if !inverse.before.targets.contains(&e.id) {
                if self.keys.contains_key(&id) {
                    left = Some(id);
                }
                continue;
            }
            // Classify this original identity independently: several old ids
            // may now resolve to one replacement. Another entry's placement
            // must not turn a missing move into a restoration.
            let place = match after.get(&e.id) {
                None => !self.keys.contains_key(&id),
                Some(other) => e.position != other.position && self.keys.contains_key(&id),
            };
            if place {
                let insert =
                    (!after.contains_key(&e.id)).then(|| remap_value(&e.value, containers));
                let element = match (self.remove(id), insert.as_ref()) {
                    (Some(element), _) => element,
                    (None, Some(value)) => Element {
                        id,
                        value: value.clone(),
                        ..e.clone()
                    },
                    // An original id and its undo-created replacement can
                    // coexist after a concurrent move revives the original.
                    // Their aliases may coincide; an absent move/set target
                    // still must not be recreated.
                    (None, None) => continue,
                };
                if let Some(LoroValue::Container(cid)) = &insert {
                    preflight.creates.insert(cid.clone());
                }
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
                        let value = remap_value(&e.value, containers);
                        if value != element.value {
                            if let LoroValue::Container(cid) = &element.value {
                                preflight.removes.insert(cid.clone());
                            }
                            if let LoroValue::Container(cid) = &value {
                                preflight.creates.insert(cid.clone());
                            }
                        }
                        element.value = value;
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
                model.undo(
                    inverse,
                    remapped,
                    containers,
                    &mut Vec::new(),
                    &mut NativePreflight::default(),
                );
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
        let current_id = remap(&cid, containers);
        let current = if inverses.iter().any(|i| i.before.full || i.after.full) {
            Snapshot::capture(doc, &current_id)
        } else {
            let scope = Scope {
                ids: inverses
                    .iter()
                    .flat_map(|i| i.before.elements.iter().chain(&i.after.elements))
                    .map(|e| remap(&e.id, remapped))
                    .collect(),
                full: false,
            };
            Snapshot::capture_span(doc, &current_id, &scope, &scope, false)
        };
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
        let mut preflight = NativePreflight::default();
        for inverse in &inverses {
            model.undo(inverse, remapped, containers, &mut edits, &mut preflight);
        }
        // This private entry advertises possible child creations to the batch
        // preflight. Positions/removals come from NativePreflight, and actual
        // application uses only the identity edits under the transaction lock.
        let delta = DeltaRopeBuilder::new().insert_many(
            preflight
                .creates
                .iter()
                .cloned()
                .map(|cid| ValueOrHandler::Value(LoroValue::Container(cid))),
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
                kind: PlanKind::Native { edits, preflight },
            },
        );
    }
    plans
}

impl Plan {
    pub(crate) fn is_native(&self) -> bool {
        matches!(self.kind, PlanKind::Native { .. })
    }

    pub(crate) fn preflight(&self) -> Option<&NativePreflight> {
        match &self.kind {
            PlanKind::Native { preflight, .. } => Some(preflight),
            _ => None,
        }
    }

    pub(crate) fn apply_with_txn(
        self,
        list: &MovableListHandler,
        txn: &mut Transaction,
        containers: &mut FxHashMap<ContainerID, ContainerID>,
        remapped: &mut ElementRemap,
    ) -> LoroResult<()> {
        let PlanKind::Native { edits, .. } = self.kind else {
            return Err(LoroError::TransactionError(
                "expected native MovableList undo plan".into(),
            ));
        };
        Self::apply_native(edits, list, txn, containers, remapped)
    }

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
            PlanKind::Native { .. } => Err(LoroError::TransactionError(
                "MovableList identity undo requires the transaction guard".into(),
            )),
        }
    }

    fn apply_native(
        edits: Vec<Edit>,
        list: &MovableListHandler,
        txn: &mut Transaction,
        containers: &mut FxHashMap<ContainerID, ContainerID>,
        remapped: &mut ElementRemap,
    ) -> LoroResult<()> {
        let index = |id: CompactIdLp, remapped: &ElementRemap| {
            let id = remap(&id, remapped);
            list.with_state(|s| {
                let state = s.as_movable_list_state().unwrap();
                Ok(state
                    .get_index_of_elem(id)
                    .filter(|i| state.get_elem_id_at(*i, IndexType::ForUser) == Some(id)))
            })
        };
        let missing =
            || LoroError::NotFoundError("MovableList undo element or anchor disappeared".into());
        for edit in edits {
            match edit {
                Edit::Delete(id) => {
                    list.delete_with_txn(txn, index(id, remapped)?.ok_or_else(missing)?, 1)?
                }
                Edit::Place {
                    id,
                    insert,
                    left,
                    right,
                } => {
                    let gap = if let Some(left) = left {
                        index(left, remapped)?.ok_or_else(missing)? + 1
                    } else if let Some(right) = right {
                        index(right, remapped)?.ok_or_else(missing)?
                    } else {
                        0
                    };
                    if let Some(value) = insert {
                        if index(id, remapped)?.is_some() {
                            return Err(LoroError::TransactionError(
                                "MovableList undo element already restored".into(),
                            ));
                        }
                        match remap_value(&value, containers) {
                            LoroValue::Container(cid) => {
                                let child = list.insert_container_with_txn(
                                    txn,
                                    gap,
                                    Handler::new_unattached(cid.container_type())?,
                                )?;
                                containers.insert(cid, child.id());
                            }
                            value => list.insert_with_txn(txn, gap, value)?,
                        }
                        let new_id = list.with_state(|s| {
                            s.as_movable_list_state()
                                .unwrap()
                                .get_elem_id_at(gap, IndexType::ForUser)
                                .ok_or_else(missing)
                        })?;
                        remapped.insert(remap(&id, remapped), new_id);
                    } else {
                        let from = index(id, remapped)?.ok_or_else(missing)?;
                        list.move_with_txn(txn, from, gap - usize::from(from < gap))?;
                    }
                }
                Edit::Set(id, value) => {
                    let index = index(id, remapped)?.ok_or_else(missing)?;
                    match remap_value(&value, containers) {
                        LoroValue::Container(cid) => {
                            if list.get(index) != Some(LoroValue::Container(cid.clone())) {
                                let child = list.set_container_with_txn(
                                    txn,
                                    index,
                                    Handler::new_unattached(cid.container_type())?,
                                )?;
                                containers.insert(cid, child.id());
                            }
                        }
                        value => list.set_with_txn(txn, index, value)?,
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static CAPTURE_STATS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_edits_keep_snapshot_and_model_sizes_bounded() {
        for (n, remove) in [
            (1_000, false),
            (10_000, false),
            (1_000, true),
            (10_000, true),
        ] {
            let doc = LoroDoc::new_auto_commit();
            let list = doc.get_movable_list("list");
            for i in 0..n {
                list.push(i.into()).unwrap();
            }
            doc.commit_then_renew();
            let undo = crate::UndoManager::new(&doc);
            if remove {
                list.delete(0, 1).unwrap();
            } else {
                list.mov(0, n as usize - 1).unwrap();
            }
            doc.commit_then_renew();
            CAPTURE_STATS.with(|stats| stats.set((0, 0)));
            assert!(undo.undo().unwrap());
            let (captured, model) = CAPTURE_STATS.with(|stats| stats.get());
            assert!(
                captured <= 12,
                "captured {captured} elements for list length {n}"
            );
            assert!(model <= 3, "model has {model} elements for list length {n}");
        }
    }
}
