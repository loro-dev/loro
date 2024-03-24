use std::{num::NonZeroU16, sync::Arc};

pub(super) mod tree;
use itertools::Itertools;

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{
    ContainerID, Counter, HasCounterSpan, HasIdSpan, IdFull, IdLp, IdSpan, LoroValue, PeerID, ID,
};
use smallvec::SmallVec;
use tracing::instrument;

use crate::{
    container::{
        idx::ContainerIdx,
        list::list_op::InnerListOp,
        richtext::{
            richtext_state::{RichtextStateChunk, TextChunk},
            AnchorType, CrdtRopeDelta, RichtextChunk, RichtextChunkValue, RichtextTracker, StyleOp,
        },
    },
    delta::{Delta, DeltaItem, ElementDelta, MapDelta, MapValue, MovableListInnerDelta},
    event::{DiffVariant, InternalDiff},
    op::{InnerContent, RichOp, SliceRange, SliceRanges},
    span::{HasId, HasLamport},
    version::Frontiers,
    InternalString, VersionVector,
};

use self::tree::TreeDiffCalculator;

use super::{event::InternalContainerDiff, oplog::OpLog};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
///
/// TODO: persist diffCalculator and skip processed version
#[derive(Debug, Default)]
pub struct DiffCalculator {
    /// ContainerIdx -> (depth, calculator)
    ///
    /// if depth is None, we need to calculate it again
    calculators: FxHashMap<ContainerIdx, (Option<NonZeroU16>, ContainerDiffCalculator)>,
    last_vv: VersionVector,
    has_all: bool,
}

impl DiffCalculator {
    pub fn new() -> Self {
        Self {
            calculators: Default::default(),
            last_vv: Default::default(),
            has_all: false,
        }
    }

    // PERF: if the causal order is linear, we can skip some of the calculation
    #[allow(unused)]
    pub(crate) fn calc_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        after: &crate::VersionVector,
    ) -> Vec<InternalContainerDiff> {
        self.calc_diff_internal(oplog, before, None, after, None)
    }

    pub(crate) fn calc_diff_internal(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        before_frontiers: Option<&Frontiers>,
        after: &crate::VersionVector,
        after_frontiers: Option<&Frontiers>,
    ) -> Vec<InternalContainerDiff> {
        let s = tracing::span!(tracing::Level::INFO, "DiffCalc");
        let _e = s.enter();
        tracing::info!("Before: {:?} After: {:?}", &before, &after);
        if self.has_all {
            let include_before = self.last_vv.includes_vv(before);
            let include_after = self.last_vv.includes_vv(after);
            if !include_after || !include_before {
                self.has_all = false;
                self.last_vv = Default::default();
            }
        }
        let affected_set = if !self.has_all {
            // if we don't have all the ops, we need to calculate the diff by tracing back
            let mut merged = before.clone();
            merged.merge(after);
            if before.is_empty() {
                self.has_all = true;
                self.last_vv = Default::default();
            }
            let (lca, iter) =
                oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);

            let mut started_set = FxHashSet::default();
            for (change, start_counter, vv) in iter {
                if change.id.counter > 0 && self.has_all {
                    assert!(
                        self.last_vv.includes_id(change.id.inc(-1)),
                        "{:?} {}",
                        &self.last_vv,
                        change.id
                    );
                }

                if self.has_all {
                    self.last_vv.extend_to_include_end_id(change.id_end());
                }

                let iter_start = change
                    .ops
                    .binary_search_by(|op| op.ctr_last().cmp(&start_counter))
                    .unwrap_or_else(|e| e);
                let mut visited = FxHashSet::default();
                for mut op in &change.ops.vec()[iter_start..] {
                    // slice the op if needed
                    let stack_sliced_op;
                    if op.counter < start_counter {
                        if op.ctr_last() < start_counter {
                            continue;
                        }

                        stack_sliced_op =
                            Some(op.slice((start_counter - op.counter) as usize, op.atom_len()));
                        op = stack_sliced_op.as_ref().unwrap();
                    }
                    let vv = &mut vv.borrow_mut();
                    vv.extend_to_include_end_id(ID::new(change.peer(), op.counter));
                    let depth = oplog.arena.get_depth(op.container);
                    let (old_depth, calculator) =
                        self.calculators.entry(op.container).or_insert_with(|| {
                            match op.container.get_type() {
                                crate::ContainerType::Text => (
                                    depth,
                                    ContainerDiffCalculator::Richtext(
                                        RichtextDiffCalculator::default(),
                                    ),
                                ),
                                crate::ContainerType::Map => (
                                    depth,
                                    ContainerDiffCalculator::Map(MapDiffCalculator::new(
                                        op.container,
                                    )),
                                ),
                                crate::ContainerType::List => (
                                    depth,
                                    ContainerDiffCalculator::List(ListDiffCalculator::default()),
                                ),
                                crate::ContainerType::Tree => (
                                    depth,
                                    ContainerDiffCalculator::Tree(TreeDiffCalculator::new(
                                        op.container,
                                    )),
                                ),
                                crate::ContainerType::MovableList => (
                                    depth,
                                    ContainerDiffCalculator::MovableList(
                                        MovableListDiffCalculator::new(op.container),
                                    ),
                                ),
                            }
                        });
                    // checkout use the same diff_calculator, the depth of calculator is not updated
                    // That may cause the container to be considered deleted
                    if *old_depth != depth {
                        *old_depth = depth;
                    }

                    if !started_set.contains(&op.container) {
                        started_set.insert(op.container);
                        calculator.start_tracking(oplog, &lca);
                    }

                    if visited.contains(&op.container) {
                        // don't checkout if we have already checked out this container in this round
                        calculator.apply_change(oplog, RichOp::new_by_change(change, op), None);
                    } else {
                        calculator.apply_change(oplog, RichOp::new_by_change(change, op), Some(vv));
                        visited.insert(op.container);
                    }
                }
            }
            for (_, (_, calculator)) in self.calculators.iter_mut() {
                calculator.stop_tracking(oplog, after);
            }

            Some(started_set)
        } else {
            // We can calculate the diff by the current calculators.

            // Find a set of affected containers idx, if it's relatively cheap
            if before.distance_to(after) < self.calculators.len() {
                let mut set = FxHashSet::default();
                oplog.for_each_change_within(before, after, |change| {
                    for op in change.ops.iter() {
                        set.insert(op.container);
                    }
                });
                Some(set)
            } else {
                None
            }
        };

        // Because we need to get correct `bring_back` value that indicates container is created during this round of diff calc,
        // we need to iterate from parents to children. i.e. from smaller depth to larger depth.
        let mut new_containers = FxHashSet::default();
        let mut container_id_to_depth = FxHashMap::default();
        let mut all: Vec<(Option<NonZeroU16>, ContainerIdx)> = if let Some(set) = affected_set {
            // only visit the affected containers
            set.into_iter()
                .map(|x| {
                    let (depth, _) = self.calculators.get_mut(&x).unwrap();
                    (*depth, x)
                })
                .collect()
        } else {
            self.calculators
                .iter_mut()
                .map(|(x, (depth, _))| (*depth, *x))
                .collect()
        };
        let mut ans = FxHashMap::default();
        while !all.is_empty() {
            // sort by depth and lamport, ensure we iterate from top to bottom
            all.sort_by_key(|x| x.0);
            for (_, idx) in std::mem::take(&mut all) {
                if ans.contains_key(&idx) {
                    continue;
                }
                let (depth, calc) = self.calculators.get_mut(&idx).unwrap();
                if depth.is_none() {
                    let d = oplog.arena.get_depth(idx);
                    if d != *depth {
                        *depth = d;
                        all.push((*depth, idx));
                        continue;
                    }
                }
                let id = oplog.arena.idx_to_id(idx).unwrap();
                let bring_back = new_containers.remove(&id);

                let diff = calc.calculate_diff(oplog, before, after, |c| {
                    new_containers.insert(c.clone());
                    container_id_to_depth.insert(c.clone(), depth.and_then(|d| d.checked_add(1)));
                    oplog.arena.register_container(c);
                });
                if !diff.is_empty() || bring_back {
                    ans.insert(
                        idx,
                        (
                            *depth,
                            InternalContainerDiff {
                                idx,
                                bring_back,
                                is_container_deleted: false,
                                diff: diff.into(),
                            },
                        ),
                    );
                }
            }
        }

        while !new_containers.is_empty() {
            for id in std::mem::take(&mut new_containers) {
                let Some(idx) = oplog.arena.id_to_idx(&id) else {
                    continue;
                };
                if ans.contains_key(&idx) {
                    continue;
                }
                let depth = container_id_to_depth.remove(&id).unwrap();
                ans.insert(
                    idx,
                    (
                        depth,
                        InternalContainerDiff {
                            idx,
                            bring_back: true,
                            is_container_deleted: false,
                            diff: DiffVariant::None,
                        },
                    ),
                );
            }
        }

        ans.into_values().map(|x| x.1).collect_vec()
    }
}

/// DiffCalculator should track the history first before it can calculate the difference.
///
/// So we need it to first apply all the ops between the two versions.
///
/// NOTE: not every op between two versions are included in a certain container.
/// So there may be some ops that cannot be seen by the container.
///
#[enum_dispatch]
pub(crate) trait DiffCalculatorTrait {
    fn start_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector);
    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    );
    fn stop_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector);
    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff;
}

#[enum_dispatch(DiffCalculatorTrait)]
#[derive(Debug)]
enum ContainerDiffCalculator {
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
    Richtext(RichtextDiffCalculator),
    Tree(TreeDiffCalculator),
    MovableList(MovableListDiffCalculator),
}

#[derive(Debug)]
struct MapDiffCalculator {
    container_idx: ContainerIdx,
    changed_key: FxHashSet<InternalString>,
}

impl MapDiffCalculator {
    pub(crate) fn new(container_idx: ContainerIdx) -> Self {
        Self {
            container_idx,
            changed_key: Default::default(),
        }
    }
}

impl DiffCalculatorTrait for MapDiffCalculator {
    fn start_tracking(&mut self, _oplog: &crate::OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        _oplog: &crate::OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        let map = op.raw_op().content.as_map().unwrap();
        self.changed_key.insert(map.key.clone());
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let mut changed = Vec::new();
        let group = oplog
            .op_groups
            .get(&self.container_idx)
            .unwrap()
            .as_map()
            .unwrap();
        for k in self.changed_key.iter() {
            let peek_from = group.last_op(k, from);
            let peek_to = group.last_op(k, to);
            match (peek_from, peek_to) {
                (None, None) => {}
                (None, Some(_)) => changed.push((k.clone(), peek_to)),
                (Some(_), None) => changed.push((k.clone(), peek_to)),
                (Some(a), Some(b)) => {
                    if a != b {
                        changed.push((k.clone(), peek_to))
                    }
                }
            }
        }

        let mut updated = FxHashMap::with_capacity_and_hasher(changed.len(), Default::default());
        for (key, value) in changed {
            let value = value
                .map(|v| {
                    let value = v.value.clone();
                    if let Some(LoroValue::Container(c)) = &value {
                        on_new_container(c);
                    }

                    MapValue {
                        value,
                        lamp: v.lamport,
                        peer: v.peer,
                    }
                })
                .unwrap_or_else(|| MapValue {
                    value: None,
                    lamp: 0,
                    peer: 0,
                });

            updated.insert(key, value);
        }
        InternalDiff::Map(MapDelta { updated })
    }
}

use rle::{HasLength, Sliceable};

#[derive(Default)]
struct ListDiffCalculator {
    start_vv: VersionVector,
    tracker: Box<RichtextTracker>,
}

impl std::fmt::Debug for ListDiffCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListDiffCalculator")
            // .field("tracker", &self.tracker)
            .finish()
    }
}

impl DiffCalculatorTrait for ListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Box::new(RichtextTracker::new_with_unknown());
            self.start_vv = vv.clone();
        }

        self.tracker.checkout(vv);
    }

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        if let Some(vv) = vv {
            self.tracker.checkout(vv);
        }

        match &op.op().content {
            crate::op::InnerContent::List(l) => match l {
                crate::container::list::list_op::InnerListOp::Insert { slice, pos } => {
                    self.tracker.insert(
                        op.id_full(),
                        *pos,
                        RichtextChunk::new_text(slice.0.clone()),
                    );
                }
                crate::container::list::list_op::InnerListOp::Delete(del) => {
                    self.tracker.delete(
                        op.id_start(),
                        del.id_start,
                        del.start() as usize,
                        del.atom_len(),
                        del.is_reversed(),
                    );
                }
                _ => unreachable!(),
            },
            crate::op::InnerContent::Map(_) => unreachable!(),
            crate::op::InnerContent::Tree(_) => unreachable!(),
        }
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let mut delta = Delta::new();
        for item in self.tracker.diff(from, to) {
            match item {
                CrdtRopeDelta::Retain(len) => {
                    delta = delta.retain(len);
                }
                CrdtRopeDelta::Insert {
                    chunk: value,
                    id,
                    lamport,
                } => match value.value() {
                    RichtextChunkValue::Text(range) => {
                        for i in range.clone() {
                            let v = oplog.arena.get_value(i as usize);
                            if let Some(LoroValue::Container(c)) = &v {
                                on_new_container(c);
                            }
                        }
                        delta = delta.insert(SliceRanges {
                            ranges: smallvec::smallvec![SliceRange(range)],
                            id: IdFull::new(id.peer, id.counter, lamport.unwrap()),
                        });
                    }
                    RichtextChunkValue::StyleAnchor { .. } => unreachable!(),
                    RichtextChunkValue::Unknown(len) => {
                        // assert not unknown id
                        delta = handle_unknown(id, oplog, len, &mut on_new_container, delta);
                    }
                    RichtextChunkValue::MoveAnchor => {
                        delta = handle_unknown(id, oplog, 1, &mut on_new_container, delta);
                    }
                },
                CrdtRopeDelta::Delete(len) => {
                    delta = delta.delete(len);
                }
            }
        }

        fn handle_unknown(
            id: ID,
            oplog: &OpLog,
            len: u32,
            on_new_container: &mut dyn FnMut(&ContainerID),
            mut delta: Delta<SliceRanges>,
        ) -> Delta<SliceRanges> {
            assert_ne!(id.peer, PeerID::MAX);
            let mut acc_len = 0;
            for rich_op in oplog.iter_ops(IdSpan::new(
                id.peer,
                id.counter,
                id.counter + len as Counter,
            )) {
                acc_len += rich_op.content_len();
                let op = rich_op.op();
                let lamport = rich_op.lamport();

                if let InnerListOp::Insert { slice, pos: _ } = op.content.as_list().unwrap() {
                    let range = slice.clone();
                    for i in slice.0.clone() {
                        let v = oplog.arena.get_value(i as usize);
                        if let Some(LoroValue::Container(c)) = &v {
                            (on_new_container)(c);
                        }
                    }

                    delta = delta.insert(SliceRanges {
                        ranges: smallvec::smallvec![range],
                        id: IdFull::new(id.peer, op.counter, lamport),
                    });
                } else if let InnerListOp::Move { .. } = op.content.as_list().unwrap() {
                    delta = delta.insert(SliceRanges {
                        // We do NOT need an actual value range,
                        // movable list container will only use the id info
                        ranges: smallvec::smallvec![SliceRange(0..1)],
                        id: IdFull::new(id.peer, op.counter, lamport),
                    });
                }
            }

            debug_assert_eq!(acc_len, len as usize);
            delta
        }

        InternalDiff::ListRaw(delta)
    }
}

#[derive(Debug, Default)]
struct RichtextDiffCalculator {
    start_vv: VersionVector,
    tracker: Box<RichtextTracker>,
    styles: Vec<StyleOp>,
}

impl DiffCalculatorTrait for RichtextDiffCalculator {
    fn start_tracking(&mut self, _oplog: &super::oplog::OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Box::new(RichtextTracker::new_with_unknown());
            self.styles.clear();
            self.start_vv = vv.clone();
        }

        self.tracker.checkout(vv);
    }

    fn apply_change(
        &mut self,
        _oplog: &super::oplog::OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        if let Some(vv) = vv {
            self.tracker.checkout(vv);
        }
        match &op.raw_op().content {
            crate::op::InnerContent::List(l) => match l {
                InnerListOp::Insert { .. } | InnerListOp::Move { .. } | InnerListOp::Set { .. } => {
                    unreachable!()
                }
                crate::container::list::list_op::InnerListOp::InsertText {
                    slice: _,
                    unicode_start,
                    unicode_len: len,
                    pos,
                } => {
                    self.tracker.insert(
                        op.id_full(),
                        *pos as usize,
                        RichtextChunk::new_text(*unicode_start..*unicode_start + *len),
                    );
                }
                crate::container::list::list_op::InnerListOp::Delete(del) => {
                    self.tracker.delete(
                        op.id_start(),
                        del.id_start,
                        del.start() as usize,
                        del.atom_len(),
                        del.is_reversed(),
                    );
                }
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    info,
                    value,
                } => {
                    debug_assert!(start < end, "start: {}, end: {}", start, end);
                    let style_id = self.styles.len();
                    self.styles.push(StyleOp {
                        lamport: op.lamport(),
                        peer: op.peer,
                        cnt: op.id_start().counter,
                        key: key.clone(),
                        value: value.clone(),
                        info: *info,
                    });
                    self.tracker.insert(
                        op.id_full(),
                        *start as usize,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::Start),
                    );
                    self.tracker.insert(
                        op.id_full().inc(1),
                        // need to shift 1 because we insert the start style anchor before this pos
                        *end as usize + 1,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::End),
                    );
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => {}
            },
            crate::op::InnerContent::Map(_) => unreachable!(),
            crate::op::InnerContent::Tree(_) => unreachable!(),
        }
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        _: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let mut delta = Delta::new();
        for item in self.tracker.diff(from, to) {
            match item {
                CrdtRopeDelta::Retain(len) => {
                    delta = delta.retain(len);
                }
                CrdtRopeDelta::Insert {
                    chunk: value,
                    id,
                    lamport,
                } => match value.value() {
                    RichtextChunkValue::Text(text) => {
                        delta = delta.insert(RichtextStateChunk::Text(
                            // PERF: can be speedup by acquiring lock on arena
                            TextChunk::new(
                                oplog
                                    .arena
                                    .slice_by_unicode(text.start as usize..text.end as usize),
                                IdFull::new(id.peer, id.counter, lamport.unwrap()),
                            ),
                        ));
                    }
                    RichtextChunkValue::StyleAnchor { id, anchor_type } => {
                        delta = delta.insert(RichtextStateChunk::Style {
                            style: Arc::new(self.styles[id as usize].clone()),
                            anchor_type,
                        });
                    }
                    RichtextChunkValue::Unknown(len) => {
                        // assert not unknown id
                        assert_ne!(id.peer, PeerID::MAX);
                        let mut acc_len = 0;
                        for rich_op in oplog.iter_ops(IdSpan::new(
                            id.peer,
                            id.counter,
                            id.counter + len as Counter,
                        )) {
                            acc_len += rich_op.content_len();
                            let op = rich_op.op();
                            let lamport = rich_op.lamport();
                            let content = op.content.as_list().unwrap();
                            match content {
                                crate::container::list::list_op::InnerListOp::InsertText {
                                    slice,
                                    ..
                                } => {
                                    delta = delta.insert(RichtextStateChunk::Text(TextChunk::new(
                                        slice.clone(),
                                        IdFull::new(id.peer, op.counter, lamport),
                                    )));
                                }
                                _ => unreachable!("{:?}", content),
                            }
                        }

                        debug_assert_eq!(acc_len, len as usize);
                    }
                    RichtextChunkValue::MoveAnchor => unreachable!(),
                },
                CrdtRopeDelta::Delete(len) => {
                    delta = delta.delete(len);
                }
            }
        }

        InternalDiff::RichtextRaw(delta)
    }
}

#[derive(Debug)]
struct MovableListDiffCalculator {
    container_idx: ContainerIdx,
    changed_elements: FxHashSet<IdLp>,
    list: ListDiffCalculator,
}

impl DiffCalculatorTrait for MovableListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(&self.list.start_vv) || !self.list.tracker.all_vv().includes_vv(vv) {
            self.list.tracker = Box::new(RichtextTracker::new_with_unknown());
            self.list.start_vv = vv.clone();
        }

        self.list.tracker.checkout(vv);
        // TODO: when can we clear the elements info?
    }

    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        let InnerContent::List(l) = &op.raw_op().content else {
            unreachable!()
        };

        // collect the elements that are moved, updated, or inserted
        match l {
            InnerListOp::Insert { slice, pos: _ } => {
                let op_id = op.id_full().idlp();
                for i in 0..slice.atom_len() {
                    let id = op_id.inc(i as Counter);

                    self.changed_elements.insert(id);
                }
            }
            InnerListOp::Delete(_) => {}
            InnerListOp::Move {
                from: _,
                from_id,
                to: _,
            } => {
                self.changed_elements.insert(*from_id);
            }
            InnerListOp::Set { elem_id, value: _ } => {
                self.changed_elements.insert(*elem_id);
            }

            InnerListOp::StyleStart { .. } => unreachable!(),
            InnerListOp::StyleEnd => unreachable!(),
            InnerListOp::InsertText { .. } => unreachable!(),
        }

        {
            // Apply change on the list items
            // TODO: it needs to ignore Set & Move op internally
            let this = &mut self.list;
            if let Some(vv) = vv {
                this.tracker.checkout(vv);
            }

            match &op.op().content {
                crate::op::InnerContent::List(l) => match l {
                    crate::container::list::list_op::InnerListOp::Insert { slice, pos } => {
                        this.tracker.insert(
                            op.id_full(),
                            *pos,
                            RichtextChunk::new_text(slice.0.clone()),
                        );
                    }
                    crate::container::list::list_op::InnerListOp::Delete(del) => {
                        this.tracker.delete(
                            op.id_start(),
                            del.id_start,
                            del.start() as usize,
                            del.atom_len(),
                            del.is_reversed(),
                        );
                    }
                    InnerListOp::Move { from, from_id, to } => {
                        let from_id = oplog.idlp_to_id(*from_id).unwrap();
                        this.tracker
                            .move_item(op.id_full(), from_id, *from as usize, *to as usize);
                    }
                    InnerListOp::Set { .. } => {
                        // don't need to update tracker here
                    }
                    InnerListOp::InsertText { .. }
                    | InnerListOp::StyleStart { .. }
                    | InnerListOp::StyleEnd => unreachable!(),
                },
                _ => unreachable!(),
            }
        };
    }

    fn stop_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector) {
        self.list.stop_tracking(oplog, vv)
    }

    #[instrument(skip_all)]
    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let InternalDiff::ListRaw(list_diff) = self.list.calculate_diff(oplog, from, to, |_| {})
        else {
            unreachable!()
        };

        let group = oplog
            .op_groups
            .get(&self.container_idx)
            .unwrap()
            .as_movable_list()
            .unwrap();
        let mut element_changes = Vec::new();
        for id in self.changed_elements.iter() {
            // It can be None if the target does not exist before the `to` version
            // But we don't need to calc from, because the deletion is handled by the diff from list items
            let Some(pos) = group.last_pos(id, to) else {
                continue;
            };

            let value = group.last_value(id, to).unwrap();
            let old_pos = group.last_pos(id, from);
            let old_value = group.last_value(id, from);
            if old_pos.is_none() && old_value.is_none() {
                if let LoroValue::Container(c) = &value.value {
                    on_new_container(c);
                }
                element_changes.push(ElementDelta::New {
                    id: *id,
                    new_pos: pos.value,
                    new_value: value.value.clone(),
                    value_id: IdLp::new(value.peer, value.lamport),
                });
            } else {
                element_changes.push(ElementDelta::Update {
                    id: *id,
                    pos: pos.value,
                    pos_updated: old_pos.unwrap().value == pos.value,
                    value: value.value.clone(),
                    value_updated: old_value.unwrap().value == value.value,
                    value_id: IdLp::new(value.peer, value.lamport),
                });
            }
        }

        let diff = MovableListInnerDelta {
            list: Delta {
                vec: (list_diff
                    .iter()
                    .map(|x| match x {
                        &DeltaItem::Retain { retain, .. } => DeltaItem::Retain {
                            retain,
                            attributes: (),
                        },
                        DeltaItem::Insert { insert, .. } => {
                            let len = insert.ranges.iter().map(|x| x.atom_len()).sum();
                            let id = insert.id;
                            let mut new_insert = SmallVec::with_capacity(len);
                            for i in 0..len {
                                new_insert.push(id.inc(i as i32));
                            }

                            DeltaItem::Insert {
                                insert: new_insert,
                                attributes: (),
                            }
                        }
                        &DeltaItem::Delete { delete, .. } => DeltaItem::Delete {
                            delete,
                            attributes: (),
                        },
                    })
                    .collect()),
            },
            elements: element_changes,
        };

        InternalDiff::MovableList(diff)
    }
}

impl MovableListDiffCalculator {
    fn new(container: ContainerIdx) -> MovableListDiffCalculator {
        MovableListDiffCalculator {
            container_idx: container,
            changed_elements: Default::default(),
            list: Default::default(),
        }
    }
}
