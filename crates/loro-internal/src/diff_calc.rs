use std::{num::NonZeroU16, sync::Arc};

#[cfg(feature = "counter")]
mod counter;
use bytes::Bytes;
#[cfg(feature = "counter")]
pub(crate) use counter::CounterDiffCalculator;
pub(super) mod tree;
mod unknown;
use itertools::Itertools;

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{
    CompactIdLp, ContainerID, Counter, HasCounterSpan, HasIdSpan, IdFull, IdLp, IdSpan, LoroValue,
    PeerID, ID,
};
use smallvec::SmallVec;
use tracing::{instrument, trace, warn};

use crate::{
    container::{
        idx::ContainerIdx,
        list::list_op::InnerListOp,
        richtext::{
            richtext_state::{RichtextStateChunk, TextChunk},
            AnchorType, CrdtRopeDelta, RichtextChunk, RichtextChunkValue, RichtextTracker, StyleOp,
        },
    },
    cursor::AbsolutePosition,
    delta::{Delta, DeltaItem, ElementDelta, MapDelta, MapValue, MovableListInnerDelta},
    event::{DiffVariant, InternalDiff},
    op::{InnerContent, RichOp, SliceRange, SliceRanges},
    span::{HasId, HasLamport},
    version::Frontiers,
    InternalString, VersionVector,
};

use self::tree::TreeDiffCalculator;

use self::unknown::UnknownDiffCalculator;

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

/// This mode defines how the diff is calculated and how it should be applied on the state.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum DiffMode {
    /// This is the most general mode of diff calculation.
    ///
    /// When applying `Checkout` diff, we already know the current state of the affected registers.
    /// So there is no need to compare the lamport values.
    ///
    /// It can be used whenever a user want to switch to a different version.
    /// But it is also the slowest mode. It relies on the `ContainerHistoryCache`, which is expensive to build and maintain in memory.
    Checkout,
    /// This mode is used when the user imports new updates.
    ///
    /// When applying `Import` diff, we may need to know the the current state.
    /// For example, we may need to compare the current register's lamport with the update's lamport to decide
    /// what's the new value.
    ///
    /// It is faster than the checkout mode, but it is still slower than the linear mode.
    ///
    /// - The difference between the import mode and the checkout mode: in import mode, target version > current version.
    ///   So when calculating the `DiffCalculator` doesn't need to rely on `ContainerHistoryCache`, except for the Tree container.
    /// - The difference between the import mode and the linear mode: in linear mode, all the imported updates are ordered, no concurrent update exists.
    ///   So there is no need to build CRDTs for the calculation
    Import,
    /// This mode is used when we don't need to build CRDTs to calculate the difference. It is the fastest mode.
    Linear,
}

impl DiffCalculator {
    pub fn new() -> Self {
        Self {
            calculators: Default::default(),
            last_vv: Default::default(),
            has_all: false,
        }
    }

    #[allow(unused)]
    pub(crate) fn get_calc(&self, container: ContainerIdx) -> Option<&ContainerDiffCalculator> {
        self.calculators.get(&container).map(|(_, c)| c)
    }

    // PERF: if the causal order is linear, we can skip some of the calculation
    #[allow(unused)]
    pub(crate) fn calc_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        after: &crate::VersionVector,
    ) -> Vec<InternalContainerDiff> {
        self.calc_diff_internal(oplog, before, None, after, None, None)
    }

    pub(crate) fn calc_diff_internal(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        before_frontiers: Option<&Frontiers>,
        after: &crate::VersionVector,
        after_frontiers: Option<&Frontiers>,
        container_filter: Option<&dyn Fn(ContainerIdx) -> bool>,
    ) -> Vec<InternalContainerDiff> {
        let s = tracing::span!(tracing::Level::INFO, "DiffCalc");
        let _e = s.enter();

        if self.has_all {
            let include_before = self.last_vv.includes_vv(before);
            let include_after = self.last_vv.includes_vv(after);
            if !include_after || !include_before {
                self.has_all = false;
                self.last_vv = Default::default();
            }
        }
        tracing::info!(
            "Before: {:?} After: {:?} has_all {}",
            &before,
            &after,
            self.has_all
        );
        let affected_set = if !self.has_all {
            // if we don't have all the ops, we need to calculate the diff by tracing back
            let mut merged = before.clone();
            merged.merge(after);
            if before.is_empty() {
                self.has_all = true;
                self.last_vv = Default::default();
            }
            let (lca, diff_mode, iter) =
                oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);
            tracing::info!("LCA: {:?}", &lca);
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

                let end_counter = merged.get(&change.id.peer).unwrap();
                if self.has_all {
                    self.last_vv.extend_to_include_end_id(change.id_end());
                }

                let iter_start = change
                    .ops
                    .binary_search_by(|op| op.ctr_last().cmp(&start_counter))
                    .unwrap_or_else(|e| e);
                let mut visited = FxHashSet::default();
                for mut op in &change.ops.vec()[iter_start..] {
                    if op.counter >= *end_counter {
                        break;
                    }

                    let idx = op.container;
                    if let Some(filter) = container_filter {
                        if !filter(idx) {
                            continue;
                        }
                    }

                    // slice the op if needed
                    // PERF: we can skip the slice by using the RichOp::new_slice
                    let stack_sliced_op;
                    if op.ctr_last() < start_counter {
                        continue;
                    }

                    if op.counter < start_counter || op.ctr_end() > *end_counter {
                        stack_sliced_op = Some(op.slice(
                            ((start_counter as usize).saturating_sub(op.counter as usize)),
                            op.atom_len().min((end_counter - op.counter) as usize),
                        ));
                        op = stack_sliced_op.as_ref().unwrap();
                    }
                    let vv = &mut vv.borrow_mut();
                    vv.extend_to_include_end_id(ID::new(change.peer(), op.counter));
                    let container = op.container;
                    let depth = oplog.arena.get_depth(container);
                    let (old_depth, calculator) = self.get_or_create_calc(container, depth);
                    // checkout use the same diff_calculator, the depth of calculator is not updated
                    // That may cause the container to be considered deleted
                    if *old_depth != depth {
                        *old_depth = depth;
                    }

                    if !started_set.contains(&op.container) {
                        started_set.insert(container);
                        calculator.start_tracking(oplog, &lca, diff_mode);
                    }

                    if visited.contains(&op.container) {
                        // don't checkout if we have already checked out this container in this round
                        calculator.apply_change(oplog, RichOp::new_by_change(&change, op), None);
                    } else {
                        calculator.apply_change(
                            oplog,
                            RichOp::new_by_change(&change, op),
                            Some(vv),
                        );
                        visited.insert(container);
                    }
                }
            }

            Some(started_set)
        } else {
            // We can calculate the diff by the current calculators.

            // Find a set of affected containers idx, if it's relatively cheap
            if before.distance_to(after) < self.calculators.len() {
                let mut set = FxHashSet::default();
                oplog.for_each_change_within(before, after, |change| {
                    for op in change.ops.iter() {
                        let idx = op.container;
                        if let Some(filter) = container_filter {
                            if !filter(idx) {
                                continue;
                            }
                        }

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
            for (_, container_idx) in std::mem::take(&mut all) {
                if ans.contains_key(&container_idx) {
                    continue;
                }
                let (depth, calc) = self.calculators.get_mut(&container_idx).unwrap();
                if depth.is_none() {
                    let d = oplog.arena.get_depth(container_idx);
                    if d != *depth {
                        *depth = d;
                        all.push((*depth, container_idx));
                        calc.finish_this_round();
                        continue;
                    }
                }
                let id = oplog.arena.idx_to_id(container_idx).unwrap();
                let bring_back = new_containers.remove(&id);

                let (diff, diff_mode) = calc.calculate_diff(oplog, before, after, |c| {
                    new_containers.insert(c.clone());
                    container_id_to_depth.insert(c.clone(), depth.and_then(|d| d.checked_add(1)));
                    oplog.arena.register_container(c);
                });
                calc.finish_this_round();
                if !diff.is_empty() || bring_back {
                    ans.insert(
                        container_idx,
                        (
                            *depth,
                            InternalContainerDiff {
                                idx: container_idx,
                                bring_back,
                                is_container_deleted: false,
                                diff: diff.into(),
                                diff_mode,
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
                            diff_mode: DiffMode::Checkout,
                        },
                    ),
                );
            }
        }

        ans.into_values().map(|x| x.1).collect_vec()
    }

    // TODO: we may remove depth info
    pub(crate) fn get_or_create_calc(
        &mut self,
        idx: ContainerIdx,
        depth: Option<NonZeroU16>,
    ) -> &mut (Option<NonZeroU16>, ContainerDiffCalculator) {
        self.calculators
            .entry(idx)
            .or_insert_with(|| match idx.get_type() {
                crate::ContainerType::Text => (
                    depth,
                    ContainerDiffCalculator::Richtext(RichtextDiffCalculator::default()),
                ),
                crate::ContainerType::Map => (
                    depth,
                    ContainerDiffCalculator::Map(MapDiffCalculator::new(idx)),
                ),
                crate::ContainerType::List => (
                    depth,
                    ContainerDiffCalculator::List(ListDiffCalculator::default()),
                ),
                crate::ContainerType::Tree => (
                    depth,
                    ContainerDiffCalculator::Tree(TreeDiffCalculator::new(idx)),
                ),
                crate::ContainerType::Unknown(_) => (
                    depth,
                    ContainerDiffCalculator::Unknown(unknown::UnknownDiffCalculator),
                ),
                crate::ContainerType::MovableList => (
                    depth,
                    ContainerDiffCalculator::MovableList(MovableListDiffCalculator::new(idx)),
                ),
                #[cfg(feature = "counter")]
                crate::ContainerType::Counter => (
                    depth,
                    ContainerDiffCalculator::Counter(CounterDiffCalculator::new(idx)),
                ),
            })
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
    fn start_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector, mode: DiffMode);
    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    );
    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode);
    /// This round of diff calc is finished, we can clear the cache
    fn finish_this_round(&mut self);
}

#[enum_dispatch(DiffCalculatorTrait)]
#[derive(Debug)]
pub(crate) enum ContainerDiffCalculator {
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
    Richtext(RichtextDiffCalculator),
    Tree(TreeDiffCalculator),
    MovableList(MovableListDiffCalculator),
    #[cfg(feature = "counter")]
    Counter(counter::CounterDiffCalculator),
    Unknown(UnknownDiffCalculator),
}

#[derive(Debug)]
pub(crate) struct MapDiffCalculator {
    container_idx: ContainerIdx,
    changed: FxHashMap<InternalString, MapValue>,
    current_mode: DiffMode,
}

impl MapDiffCalculator {
    pub(crate) fn new(container_idx: ContainerIdx) -> Self {
        Self {
            container_idx,
            changed: Default::default(),
            current_mode: DiffMode::Checkout,
        }
    }
}

impl DiffCalculatorTrait for MapDiffCalculator {
    fn start_tracking(
        &mut self,
        _oplog: &crate::OpLog,
        _vv: &crate::VersionVector,
        mode: DiffMode,
    ) {
        self.changed.clear();
        self.current_mode = mode;
    }

    fn apply_change(
        &mut self,
        _oplog: &crate::OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        if matches!(self.current_mode, DiffMode::Checkout) {
            // We need to use history cache anyway
            return;
        }

        let map = op.raw_op().content.as_map().unwrap();
        let new_value = MapValue {
            value: map.value.clone(),
            peer: op.peer,
            lamp: op.lamport(),
        };
        match self.changed.get(&map.key) {
            Some(old_value) if old_value > &new_value => {}
            _ => {
                self.changed.insert(map.key.clone(), new_value);
            }
        }
    }

    fn finish_this_round(&mut self) {
        self.changed.clear();
        self.current_mode = DiffMode::Checkout;
    }

    fn calculate_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        match self.current_mode {
            DiffMode::Checkout => {
                let mut changed = Vec::new();
                let group = oplog
                    .op_groups
                    .get(&self.container_idx)
                    .unwrap()
                    .as_map()
                    .unwrap();
                for k in group.keys() {
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

                let mut updated =
                    FxHashMap::with_capacity_and_hasher(changed.len(), Default::default());
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

                (InternalDiff::Map(MapDelta { updated }), DiffMode::Checkout)
            }
            DiffMode::Import | DiffMode::Linear => {
                let changed = std::mem::take(&mut self.changed);
                let mode = self.current_mode;
                // Reset this field to avoid we use `has_all` to cache the diff calc and use it next round
                // (In the next round we need to use the checkout mode)
                self.current_mode = DiffMode::Checkout;
                (InternalDiff::Map(MapDelta { updated: changed }), mode)
            }
        }
    }
}

use rle::{HasLength, Sliceable};

#[derive(Default)]
pub(crate) struct ListDiffCalculator {
    start_vv: VersionVector,
    tracker: Box<RichtextTracker>,
}

impl ListDiffCalculator {
    pub(crate) fn get_id_latest_pos(&self, id: ID) -> Option<crate::cursor::AbsolutePosition> {
        self.tracker.get_target_id_latest_index_at_new_version(id)
    }
}

impl MovableListDiffCalculator {
    pub(crate) fn get_id_latest_pos(&self, id: ID) -> Option<crate::cursor::AbsolutePosition> {
        self.list
            .tracker
            .get_target_id_latest_index_at_new_version(id)
    }
}

impl std::fmt::Debug for ListDiffCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListDiffCalculator")
            // .field("tracker", &self.tracker)
            .finish()
    }
}

impl DiffCalculatorTrait for ListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector, mode: DiffMode) {
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
            _ => unreachable!(),
        }
    }

    fn finish_this_round(&mut self) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
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

        /// Handle span with unknown content when calculating diff
        ///
        /// We can lookup the content of the span by the id in the oplog
        fn handle_unknown(
            id: ID,
            oplog: &OpLog,
            len: u32,
            on_new_container: &mut dyn FnMut(&ContainerID),
            mut delta: Delta<SliceRanges>,
        ) -> Delta<SliceRanges> {
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

        (InternalDiff::ListRaw(delta), DiffMode::Checkout)
    }
}

#[derive(Debug, Default)]
pub(crate) struct RichtextDiffCalculator {
    start_vv: VersionVector,
    tracker: Box<RichtextTracker>,
    styles: Vec<StyleOp>,
}

impl RichtextDiffCalculator {
    /// This should be called after calc_diff
    ///
    /// TODO: Refactor, this can be simplified
    pub fn get_id_latest_pos(&self, id: ID) -> Option<AbsolutePosition> {
        self.tracker.get_target_id_latest_index_at_new_version(id)
    }
}

impl DiffCalculatorTrait for RichtextDiffCalculator {
    fn start_tracking(
        &mut self,
        _oplog: &super::oplog::OpLog,
        vv: &crate::VersionVector,
        mode: DiffMode,
    ) {
        if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Box::new(RichtextTracker::new_with_unknown());
            self.styles.clear();
            self.start_vv = vv.clone();
        }

        self.tracker.checkout(vv);
    }

    fn apply_change(
        &mut self,
        oplog: &super::oplog::OpLog,
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
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => {
                    let id = op.id();
                    // PERF: this can be sped up by caching the last style op
                    let start_op = oplog.get_op(op.id().inc(-1)).unwrap();
                    let InnerListOp::StyleStart {
                        start: _,
                        end,
                        key,
                        value,
                        info,
                    } = start_op.content.as_list().unwrap()
                    else {
                        unreachable!()
                    };
                    let style_id = match self.styles.last() {
                        Some(last) if last.peer == id.peer && last.cnt == id.counter - 1 => {
                            self.styles.len() - 1
                        }
                        _ => {
                            self.styles.push(StyleOp {
                                lamport: op.lamport() - 1,
                                peer: id.peer,
                                cnt: id.counter - 1,
                                key: key.clone(),
                                value: value.clone(),
                                info: *info,
                            });
                            self.styles.len() - 1
                        }
                    };
                    self.tracker.insert(
                        op.id_full(),
                        // need to shift 1 because we insert the start style anchor before this pos
                        *end as usize + 1,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::End),
                    );
                }
            },
            _ => unreachable!(),
        }
    }

    fn finish_this_round(&mut self) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        _: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        // TODO: PERF: It can be linearized in certain cases
        tracing::debug!("CalcDiff {:?} {:?}", from, to);
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

        (InternalDiff::RichtextRaw(delta), DiffMode::Checkout)
    }
}

#[derive(Debug)]
pub(crate) struct MovableListDiffCalculator {
    container_idx: ContainerIdx,
    changed_elements: FxHashSet<IdLp>,
    list: ListDiffCalculator,
}

impl DiffCalculatorTrait for MovableListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector, mode: DiffMode) {
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
                elem_id: from_id,
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

            let real_op = op.op();
            match &real_op.content {
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
                    InnerListOp::Move {
                        from,
                        elem_id: from_id,
                        to,
                    } => {
                        // TODO: PERF: this lookup can be optimized
                        let list = oplog
                            .op_groups
                            .get(&real_op.container)
                            .unwrap()
                            .as_movable_list()
                            .unwrap();
                        let last_pos = list
                            .last_pos(from_id, this.tracker.current_vv())
                            .unwrap()
                            .id();
                        this.tracker.move_item(
                            op.id_full(),
                            last_pos,
                            *from as usize,
                            *to as usize,
                        );
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

    fn finish_this_round(&mut self) {
        self.list.finish_this_round()
    }

    #[instrument(skip(self, oplog, on_new_container))]
    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        let (InternalDiff::ListRaw(list_diff), diff_mode) =
            self.list.calculate_diff(oplog, from, to, |_| {})
        else {
            unreachable!()
        };

        assert_eq!(diff_mode, DiffMode::Checkout);
        let group = oplog
            .op_groups
            .get_movable_list(&self.container_idx)
            .unwrap();

        let mut element_changes: FxHashMap<CompactIdLp, ElementDelta> =
            FxHashMap::with_capacity_and_hasher(
                self.changed_elements.len() + list_diff.insert_len(),
                Default::default(),
            );
        for id in self.changed_elements.iter() {
            element_changes.insert(id.compact(), ElementDelta::placeholder());
        }
        let list_diff: Delta<SmallVec<[IdFull; 1]>, ()> = Delta {
            vec: list_diff
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
                            let id = id.inc(i as i32);
                            let op = oplog.get_op(id.id()).unwrap();
                            let elem_id = match op.content.as_list().unwrap() {
                                InnerListOp::Insert { .. } => id.idlp().compact(),
                                InnerListOp::Move { elem_id, .. } => elem_id.compact(),
                                _ => unreachable!(),
                            };

                            // add the related element id
                            element_changes.insert(elem_id, ElementDelta::placeholder());
                            new_insert.push(id);
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
                .collect(),
        };

        element_changes.retain(|id, change| {
            let id = id.to_id();
            // It can be None if the target does not exist before the `to` version
            // But we don't need to calc from, because the deletion is handled by the diff from list items
            let Some(pos) = group.last_pos(&id, to) else {
                return false;
            };

            let value = group.last_value(&id, to).unwrap();
            let old_pos = group.last_pos(&id, from);
            let old_value = group.last_value(&id, from);
            if old_pos.is_none() && old_value.is_none() {
                if let LoroValue::Container(c) = &value.value {
                    on_new_container(c);
                }
                *change = ElementDelta {
                    pos: Some(pos.idlp()),
                    value: value.value.clone(),
                    value_id: Some(IdLp::new(value.peer, value.lamport)),
                    value_updated: true,
                };
            } else {
                // TODO: PERF: can be filtered based on the list_diff and whether the pos/value are updated
                *change = ElementDelta {
                    pos: Some(pos.idlp()),
                    value: value.value.clone(),
                    value_updated: old_value.unwrap().value != value.value,
                    value_id: Some(IdLp::new(value.peer, value.lamport)),
                };
            }

            true
        });

        let diff = MovableListInnerDelta {
            list: list_diff,
            elements: element_changes,
        };

        (InternalDiff::MovableList(diff), DiffMode::Checkout)
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
