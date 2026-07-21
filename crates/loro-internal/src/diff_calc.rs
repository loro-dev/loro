use std::{num::NonZeroU16, sync::Arc};

#[cfg(feature = "counter")]
mod counter;
#[cfg(feature = "counter")]
pub(crate) use counter::CounterDiffCalculator;
pub(super) mod tree;
mod unknown;
use either::Either;
use generic_btree::rle::HasLength as _;
use itertools::Itertools;

use enum_dispatch::enum_dispatch;
use loro_common::{
    CompactIdLp, ContainerID, Counter, HasCounterSpan, IdFull, IdLp, IdSpan, LoroValue, PeerID, ID,
};
use loro_delta::DeltaRope;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use tracing::{info_span, instrument};

use crate::{
    change::Lamport,
    container::{
        idx::ContainerIdx,
        list::list_op::InnerListOp,
        richtext::{
            richtext_state::{RichtextStateChunk, TextChunk},
            AnchorType, CrdtRopeDelta, RichtextChunk, RichtextChunkValue, RichtextTracker, StyleOp,
        },
    },
    cursor::AbsolutePosition,
    delta::{
        Delta, DeltaItem, DeltaValue, ElementDelta, MapDelta, MapValue, MovableListInnerDelta,
    },
    event::{DiffVariant, InternalDiff},
    op::{InnerContent, RichOp, SliceRange, SliceWithId},
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
#[derive(Debug)]
pub struct DiffCalculator {
    /// ContainerIdx -> (depth, calculator)
    ///
    /// if depth is None, we need to calculate it again
    calculators: FxHashMap<ContainerIdx, (Option<NonZeroU16>, ContainerDiffCalculator)>,
    retain_mode: DiffCalculatorRetainMode,
}

#[derive(Debug)]
enum DiffCalculatorRetainMode {
    /// The diff calculator can only be used once.
    Once { used: bool },
    /// The diff calculator will be persisted and can be reused after the diff calc is done.
    Persist,
}

/// This mode defines how the diff is calculated and how it should be applied on the state.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum DiffMode {
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
    /// It has stricter requirements than `Checkout`:
    ///
    /// - The target version vector must be greater than the current version vector.
    Import,
    /// This mode is used when the user imports new updates and all the updates are guaranteed to greater than the current version.
    ///
    /// It has stricter requirements than `Import`.
    /// - All the updates are greater than the current version. No update is concurrent to the current version.
    /// - So LCA is always the `from` version
    ImportGreaterUpdates,
    /// This mode is used when we don't need to build CRDTs to calculate the difference. It is the fastest mode.
    ///
    /// It has stricter requirements than `ImportGreaterUpdates`.
    /// - In `ImportGreaterUpdates`, all the updates are guaranteed to be greater than the current version.
    /// - In `Linear`, all the updates are ordered, no concurrent update exists.
    Linear,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DiffCalcVersionInfo<'a> {
    from_vv: &'a VersionVector,
    to_vv: &'a VersionVector,
    from_frontiers: &'a Frontiers,
    to_frontiers: &'a Frontiers,
}

impl DiffCalculator {
    /// Create a new diff calculator.
    ///
    /// If `persist` is true, the diff calculator will be persisted after the diff calc is done.
    /// This is useful when we need to cache the diff calculator for future use. But it is slower
    /// for importing updates and requires more memory.
    pub fn new(persist: bool) -> Self {
        Self {
            calculators: Default::default(),
            retain_mode: if persist {
                DiffCalculatorRetainMode::Persist
            } else {
                DiffCalculatorRetainMode::Once { used: false }
            },
        }
    }

    #[allow(unused)]
    pub(crate) fn get_calc(&self, container: ContainerIdx) -> Option<&ContainerDiffCalculator> {
        self.calculators.get(&container).map(|(_, c)| c)
    }

    /// Calculate the diff between two versions.
    ///
    /// Return the diff and the origin diff mode (it's not the diff mode used by the diff calculator.
    /// It's the expected diff mode inferred from the two version, which can reflect the direction of the
    /// change).
    pub(crate) fn calc_diff_internal(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        before_frontiers: &Frontiers,
        after: &crate::VersionVector,
        after_frontiers: &Frontiers,
        container_filter: Option<&dyn Fn(ContainerIdx) -> bool>,
    ) -> (Vec<InternalContainerDiff>, DiffMode) {
        if before == after {
            return (Vec::new(), DiffMode::Linear);
        }

        let s = tracing::span!(tracing::Level::INFO, "DiffCalc", ?before, ?after,);
        let _e = s.enter();

        let mut merged = before.clone();
        merged.merge(after);
        let (lca, origin_diff_mode, iter) =
            oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);
        let mut diff_mode = origin_diff_mode;
        match &mut self.retain_mode {
            DiffCalculatorRetainMode::Once { used } => {
                if *used {
                    panic!("DiffCalculator with retain_mode Once can only be used once");
                }
            }
            DiffCalculatorRetainMode::Persist => {
                diff_mode = DiffMode::Checkout;
            }
        }

        let affected_set = {
            loro_common::debug!("LCA: {:?} mode={:?}", &lca, diff_mode);
            let mut started_set = FxHashSet::default();
            for (change, (start_counter, end_counter), vv) in iter {
                let iter_start = change
                    .ops
                    .binary_search_by(|op| op.ctr_last().cmp(&start_counter))
                    .unwrap_or_else(|e| e);
                let mut visited = FxHashSet::default();
                for mut op in &change.ops.vec()[iter_start..] {
                    if op.counter >= end_counter {
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

                    if op.counter < start_counter || op.ctr_end() > end_counter {
                        stack_sliced_op = Some(op.slice(
                            (start_counter as usize).saturating_sub(op.counter as usize),
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
        let info = DiffCalcVersionInfo {
            from_vv: before,
            to_vv: after,
            from_frontiers: before_frontiers,
            to_frontiers: after_frontiers,
        };
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
                        continue;
                    }
                }
                let id = oplog.arena.idx_to_id(container_idx).unwrap();
                let bring_back = new_containers.remove(&id);

                info_span!("CalcDiff", ?id).in_scope(|| {
                    let (diff, diff_mode) = calc.calculate_diff(container_idx, oplog, info, |c| {
                        new_containers.insert(c.clone());
                        container_id_to_depth
                            .insert(c.clone(), depth.and_then(|d| d.checked_add(1)));
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
                                    diff: diff.into(),
                                    diff_mode,
                                },
                            ),
                        );
                    }
                });
            }
        }

        while !new_containers.is_empty() {
            for id in std::mem::take(&mut new_containers) {
                // Registration can be lazy; ensure it is registered so we can proceed
                let idx = oplog.arena.register_container(&id);
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
                            diff: DiffVariant::None,
                            diff_mode: DiffMode::Checkout,
                        },
                    ),
                );
            }
        }

        (
            ans.into_values().map(|x| x.1).collect_vec(),
            origin_diff_mode,
        )
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
                    ContainerDiffCalculator::Richtext(RichtextDiffCalculator::new()),
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
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
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
    changed: FxHashMap<InternalString, Option<MapValue>>,
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
            Some(Some(old_value)) if old_value > &new_value => {}
            _ => {
                self.changed.insert(map.key.clone(), Some(new_value));
            }
        }
    }

    fn finish_this_round(&mut self) {
        self.changed.clear();
        self.current_mode = DiffMode::Checkout;
    }

    fn calculate_diff(
        &mut self,
        _idx: ContainerIdx,
        oplog: &super::oplog::OpLog,
        DiffCalcVersionInfo { from_vv, to_vv, .. }: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        match self.current_mode {
            DiffMode::Checkout | DiffMode::Import => oplog.with_history_cache(|h| {
                let checkout_index = &h.get_checkout_index().map;
                let mut changed = Vec::new();
                let from_map = checkout_index.get_container_latest_op_at_vv(
                    self.container_idx,
                    from_vv,
                    Lamport::MAX,
                    oplog,
                );
                let mut to_map = checkout_index.get_container_latest_op_at_vv(
                    self.container_idx,
                    to_vv,
                    Lamport::MAX,
                    oplog,
                );

                for (k, peek_from) in from_map.iter() {
                    let peek_to = to_map.remove(k);
                    match peek_to {
                        None => changed.push((k.clone(), None)),
                        Some(b) => {
                            if peek_from.value != b.value {
                                changed.push((k.clone(), Some(b)))
                            }
                        }
                    }
                }

                for (k, peek_to) in to_map.into_iter() {
                    changed.push((k, Some(peek_to)));
                }

                let mut updated =
                    FxHashMap::with_capacity_and_hasher(changed.len(), Default::default());
                for (key, value) in changed {
                    let value = value.map(|v| {
                        let value = v.value.clone();
                        if let Some(LoroValue::Container(c)) = &value {
                            on_new_container(c);
                        }

                        MapValue {
                            value,
                            lamp: v.lamport,
                            peer: v.peer,
                        }
                    });

                    updated.insert(key, value);
                }

                (InternalDiff::Map(MapDelta { updated }), DiffMode::Checkout)
            }),
            DiffMode::ImportGreaterUpdates | DiffMode::Linear => {
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

use rle::{HasLength as _, Sliceable};

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
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector, _mode: DiffMode) {
        if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            *self.tracker = RichtextTracker::new_with_unknown();
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
                InnerListOp::Insert { slice, pos } => {
                    self.tracker.insert(
                        op.id_full(),
                        *pos,
                        RichtextChunk::new_text(slice.0.clone()),
                    );
                }
                InnerListOp::Delete(del) => {
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
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        let mut delta = Delta::new();
        for item in self.tracker.diff(info.from_vv, info.to_vv) {
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
                        delta = delta.insert(SliceWithId {
                            values: Either::Left(SliceRange(range)),
                            id: IdFull::new(id.peer, id.counter, lamport.unwrap()),
                            elem_id: None,
                        });
                    }
                    RichtextChunkValue::StyleAnchor { .. } => unreachable!(),
                    RichtextChunkValue::Unknown(len) => {
                        delta = handle_unknown(idx, id, oplog, len, &mut on_new_container, delta);
                    }
                    RichtextChunkValue::MoveAnchor => {
                        delta = handle_unknown(idx, id, oplog, 1, &mut on_new_container, delta);
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
            idx: ContainerIdx,
            mut id: ID,
            oplog: &OpLog,
            len: u32,
            on_new_container: &mut dyn FnMut(&ContainerID),
            mut delta: Delta<SliceWithId>,
        ) -> Delta<SliceWithId> {
            // assert not unknown id
            assert_ne!(id.peer, PeerID::MAX);
            let mut acc_len = 0;
            let end = id.counter + len as Counter;
            let shallow_root = oplog.shallow_since_vv().get(&id.peer).copied().unwrap_or(0);
            if id.counter < shallow_root {
                // need to find the content between id.counter ~ target_end in gc state
                let target_end = shallow_root.min(end);
                delta = oplog.with_history_cache(|h| {
                    let chunks =
                        h.find_list_chunks_in(idx, IdSpan::new(id.peer, id.counter, target_end));
                    for c in chunks {
                        acc_len += c.length();
                        match &c.values {
                            Either::Left(_) => unreachable!(),
                            Either::Right(r) => {
                                if let LoroValue::Container(c) = r {
                                    on_new_container(c)
                                }
                            }
                        }
                        delta = delta.insert(c);
                    }

                    delta
                });
                id.counter = shallow_root;
            }

            if id.counter < end {
                for rich_op in oplog.iter_ops(IdSpan::new(id.peer, id.counter, end)) {
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

                        delta = delta.insert(SliceWithId {
                            values: Either::Left(range),
                            id: IdFull::new(id.peer, op.counter, lamport),
                            elem_id: None,
                        });
                    } else if let InnerListOp::Move { elem_id, .. } = op.content.as_list().unwrap()
                    {
                        delta = delta.insert(SliceWithId {
                            // We do NOT need an actual value range,
                            // movable list container will only use the id info
                            values: Either::Right(LoroValue::Null),
                            id: IdFull::new(id.peer, op.counter, lamport),
                            elem_id: Some(elem_id.compact()),
                        });
                    }
                }
            }

            debug_assert_eq!(acc_len, len as usize);
            delta
        }

        (InternalDiff::ListRaw(delta), DiffMode::Checkout)
    }
}

#[derive(Debug)]
pub(crate) struct RichtextDiffCalculator {
    mode: Box<RichtextCalcMode>,
}

#[derive(Debug)]
enum RichtextCalcMode {
    Crdt {
        tracker: Box<RichtextTracker>,
        /// (op, end_pos)
        styles: Vec<(StyleOp, usize)>,
        start_vv: VersionVector,
    },
    Linear {
        diff: DeltaRope<RichtextStateChunk, ()>,
        last_style_start: Option<(Arc<StyleOp>, u32)>,
    },
}

impl RichtextDiffCalculator {
    pub fn new() -> Self {
        Self {
            mode: Box::new(RichtextCalcMode::Crdt {
                tracker: Box::new(RichtextTracker::new_with_unknown()),
                styles: Vec::new(),
                start_vv: VersionVector::new(),
            }),
        }
    }

    /// This should be called after calc_diff
    ///
    /// TODO: Refactor, this can be simplified
    pub fn get_id_latest_pos(&self, id: ID) -> Option<AbsolutePosition> {
        match &*self.mode {
            RichtextCalcMode::Crdt { tracker, .. } => {
                tracker.get_target_id_latest_index_at_new_version(id)
            }
            RichtextCalcMode::Linear { .. } => unreachable!(),
        }
    }

    /// Compare the tombstone-stable total order of two insertion ids on the
    /// tracker built by this calculator. Should be called after calc_diff.
    ///
    /// Returns `None` when either id cannot be resolved, or when the calculator
    /// is in linear mode (which keeps no tracker).
    #[cfg(feature = "persistent-anchor-tracker")]
    pub fn compare_ids(&self, a: ID, b: ID) -> Option<std::cmp::Ordering> {
        match &*self.mode {
            RichtextCalcMode::Crdt { tracker, .. } => tracker.compare_ids(a, b),
            RichtextCalcMode::Linear { .. } => None,
        }
    }

    /// Seam-rank of an insertion id on the tracker built by this calculator.
    /// Should be called after the tracker is settled at head. Returns `None`
    /// when the id cannot be resolved, or in linear mode (which keeps no tracker).
    #[cfg(feature = "persistent-anchor-tracker")]
    pub fn id_to_activated_index(&self, id: ID) -> Option<AbsolutePosition> {
        match &*self.mode {
            RichtextCalcMode::Crdt { tracker, .. } => tracker.id_to_activated_index(id),
            RichtextCalcMode::Linear { .. } => None,
        }
    }
}

/// How many times a persistent per-container text tracker was built from
/// genesis, and how many incremental advances were performed. Used only by the
/// cache-effectiveness tests to prove queries at one version cost one build and
/// zero advances, and a query after new ops costs one advance and no rebuild.
#[cfg(feature = "persistent-anchor-tracker")]
pub(crate) static TEXT_TRACKER_BUILD_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "persistent-anchor-tracker")]
pub(crate) static TEXT_TRACKER_ADVANCE_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// When set, the debug-only genesis cross-check runs on every query rather than
/// being sampled. Tests that want to exercise it on every call force it on.
#[cfg(feature = "persistent-anchor-tracker")]
pub(crate) static FORCE_GENESIS_CHECK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Rolling call counter for sampling the genesis cross-check.
#[cfg(feature = "persistent-anchor-tracker")]
static GENESIS_CHECK_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Run the O(history) genesis cross-check on the first call and then on one call
/// in this many. The cheap settle and length invariants still run on every
/// query; only the full rebuild is sampled, so a debug build issuing frequent
/// queries on a large document does not pay a rebuild every time.
#[cfg(feature = "persistent-anchor-tracker")]
const GENESIS_CHECK_SAMPLE_RATE: usize = 32;

/// A warm per-container richtext tracker, kept settled at `built_at` and
/// advanced forward pull-on-query. Homing the whole [`RichtextDiffCalculator`]
/// (not a bare tracker) keeps its style registry and start version alongside the
/// tracker, so the op-to-tracker translation is reused verbatim.
#[cfg(feature = "persistent-anchor-tracker")]
#[derive(Debug)]
pub(crate) struct CachedTextTracker {
    calc: RichtextDiffCalculator,
    built_at: Frontiers,
    built_at_vv: VersionVector,
}

#[cfg(feature = "persistent-anchor-tracker")]
impl CachedTextTracker {
    /// Build a tracker from genesis for `idx`, settled at the current oplog head.
    pub(crate) fn build(oplog: &OpLog, idx: ContainerIdx) -> Self {
        TEXT_TRACKER_BUILD_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut calc = RichtextDiffCalculator::new();
        calc.advance_to(oplog, idx, &VersionVector::new(), &Frontiers::default());
        Self {
            calc,
            built_at: oplog.frontiers().clone(),
            built_at_vv: oplog.vv().clone(),
        }
    }

    /// Advance the tracker to the current oplog head if it has moved. Keyed on
    /// the version rather than a per-container applied version, which is only a
    /// subset on multi-container documents.
    pub(crate) fn ensure_advanced(&mut self, oplog: &OpLog, idx: ContainerIdx) {
        if self.built_at_vv == *oplog.vv() {
            return;
        }
        TEXT_TRACKER_ADVANCE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let from_vv = std::mem::take(&mut self.built_at_vv);
        let from_frontiers = std::mem::take(&mut self.built_at);
        self.calc.advance_to(oplog, idx, &from_vv, &from_frontiers);
        self.built_at = oplog.frontiers().clone();
        self.built_at_vv = oplog.vv().clone();
    }

    pub(crate) fn compare_ids(&self, a: ID, b: ID) -> Option<std::cmp::Ordering> {
        self.calc.compare_ids(a, b)
    }

    pub(crate) fn id_to_activated_index(&self, id: ID) -> Option<AbsolutePosition> {
        self.calc.id_to_activated_index(id)
    }

    /// Debug-only coherence checks run after the tracker is settled at head.
    pub(crate) fn debug_check(
        &self,
        oplog: &OpLog,
        idx: ContainerIdx,
        expected_entity_len: Option<usize>,
    ) {
        if !cfg!(debug_assertions) {
            return;
        }
        self.calc.debug_check_settled(oplog.vv());
        if let Some(expected) = expected_entity_len {
            self.calc.debug_check_activated_len(expected);
        }
        let forced = FORCE_GENESIS_CHECK.load(std::sync::atomic::Ordering::Relaxed);
        let n = GENESIS_CHECK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if forced || n.is_multiple_of(GENESIS_CHECK_SAMPLE_RATE) {
            self.calc.debug_check_matches_genesis(oplog, idx);
        }
    }
}

#[cfg(feature = "persistent-anchor-tracker")]
impl RichtextDiffCalculator {
    /// Replay `[from .. oplog head]` into the tracker, then settle it at head.
    ///
    /// Ops are applied through the same causal traversal the diff path uses:
    /// each op is checked out to its own dependency version before it is
    /// inserted or deleted, which is what keeps concurrent orderings correct.
    /// The first op of a container within a change carries that dependency
    /// version; later ops of the same change reuse the already-established
    /// checkout.
    ///
    /// Unlike the diff path this does not call `start_tracking`: that helper's
    /// rebuild guard compares the tracker's per-container applied version to the
    /// whole-document version and would discard the warm tracker (and its
    /// resolvable insert ids) on any multi-container document. The persistent
    /// tracker only ever moves forward, so no rebuild-to-a-later-baseline is
    /// wanted; the closing `checkout` forwards every retreated branch to head
    /// without entering diff mode.
    fn advance_to(
        &mut self,
        oplog: &OpLog,
        idx: ContainerIdx,
        from: &VersionVector,
        from_frontiers: &Frontiers,
    ) {
        let to = oplog.vv();
        let to_frontiers = oplog.frontiers();
        let (_lca, _origin_diff_mode, iter) =
            oplog.iter_from_lca_causally(from, from_frontiers, to, to_frontiers);
        for (change, (start_counter, end_counter), vv) in iter {
            let iter_start = change
                .ops
                .binary_search_by(|op| op.ctr_last().cmp(&start_counter))
                .unwrap_or_else(|e| e);
            let mut visited = false;
            for mut op in &change.ops.vec()[iter_start..] {
                if op.counter >= end_counter {
                    break;
                }

                if op.container != idx {
                    continue;
                }

                let stack_sliced_op;
                if op.ctr_last() < start_counter {
                    continue;
                }

                if op.counter < start_counter || op.ctr_end() > end_counter {
                    stack_sliced_op = Some(op.slice(
                        (start_counter as usize).saturating_sub(op.counter as usize),
                        op.atom_len().min((end_counter - op.counter) as usize),
                    ));
                    op = stack_sliced_op.as_ref().unwrap();
                }

                let vv = &mut vv.borrow_mut();
                vv.extend_to_include_end_id(ID::new(change.peer(), op.counter));
                if visited {
                    self.apply_change(oplog, RichOp::new_by_change(&change, op), None);
                } else {
                    self.apply_change(oplog, RichOp::new_by_change(&change, op), Some(vv));
                    visited = true;
                }
            }
        }

        // Settle: forward all retreated branches to head without setting diff
        // status, so a later query reads apply-time positions rather than a
        // half-finished diff.
        self.checkout_tracker(to);
    }

    fn checkout_tracker(&mut self, vv: &VersionVector) {
        if let RichtextCalcMode::Crdt { tracker, .. } = &mut *self.mode {
            tracker.checkout(vv);
        }
    }

    fn tracker(&self) -> Option<&RichtextTracker> {
        match &*self.mode {
            RichtextCalcMode::Crdt { tracker, .. } => Some(&**tracker),
            RichtextCalcMode::Linear { .. } => None,
        }
    }

    fn debug_check_settled(&self, oplog_vv: &VersionVector) {
        if let Some(tracker) = self.tracker() {
            debug_assert_eq!(
                tracker.current_vv(),
                oplog_vv,
                "persistent text tracker is not settled at head"
            );
            debug_assert_eq!(
                tracker.changed_num(),
                0,
                "persistent text tracker was left mid-diff"
            );
        }
    }

    fn debug_check_activated_len(&self, expected: usize) {
        if let Some(tracker) = self.tracker() {
            debug_assert_eq!(
                tracker.activated_entity_len(),
                expected,
                "persistent text tracker activated length diverged from the state entity length"
            );
        }
    }

    fn debug_check_matches_genesis(&self, oplog: &OpLog, idx: ContainerIdx) {
        let mut oracle = RichtextDiffCalculator::new();
        oracle.advance_to(oplog, idx, &VersionVector::new(), &Frontiers::default());
        debug_assert_eq!(
            self.tracker().map(|t| t.normalized_entities()),
            oracle.tracker().map(|t| t.normalized_entities()),
            "incremental text tracker diverged from a fresh genesis rebuild"
        );
    }
}

impl DiffCalculatorTrait for RichtextDiffCalculator {
    fn start_tracking(
        &mut self,
        _oplog: &super::oplog::OpLog,
        vv: &crate::VersionVector,
        mode: DiffMode,
    ) {
        match mode {
            DiffMode::Linear => {
                *self.mode = RichtextCalcMode::Linear {
                    diff: DeltaRope::new(),
                    last_style_start: None,
                };
            }
            _ => {
                if !matches!(&*self.mode, RichtextCalcMode::Crdt { .. }) {
                    unreachable!();
                }
            }
        }

        match &mut *self.mode {
            RichtextCalcMode::Crdt {
                tracker,
                styles,
                start_vv,
            } => {
                if !vv.includes_vv(start_vv) || !tracker.all_vv().includes_vv(vv) {
                    **tracker = RichtextTracker::new_with_unknown();
                    styles.clear();
                    *start_vv = vv.clone();
                }

                tracker.checkout(vv);
            }
            RichtextCalcMode::Linear { .. } => {}
        }
    }

    fn apply_change(
        &mut self,
        oplog: &super::oplog::OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        match &mut *self.mode {
            RichtextCalcMode::Linear {
                diff,
                last_style_start,
            } => match &op.raw_op().content {
                crate::op::InnerContent::List(l) => match l {
                    InnerListOp::Insert { .. }
                    | InnerListOp::Move { .. }
                    | InnerListOp::Set { .. } => {
                        unreachable!()
                    }
                    InnerListOp::InsertText {
                        slice: _,
                        unicode_start,
                        unicode_len: len,
                        pos,
                    } => {
                        let s = oplog.arena.slice_by_unicode(
                            *unicode_start as usize..(*unicode_start + *len) as usize,
                        );
                        diff.insert_value(
                            *pos as usize,
                            RichtextStateChunk::new_text(s, op.id_full()),
                            (),
                        );
                    }
                    InnerListOp::Delete(del) => {
                        diff.delete(del.start() as usize, del.atom_len());
                    }
                    InnerListOp::StyleStart {
                        start,
                        end,
                        key,
                        info,
                        value,
                    } => {
                        debug_assert!(start < end, "start: {}, end: {}", start, end);
                        let style_op = Arc::new(StyleOp {
                            lamport: op.lamport(),
                            peer: op.peer,
                            cnt: op.id_start().counter,
                            key: key.clone(),
                            value: value.clone(),
                            info: *info,
                        });

                        *last_style_start = Some((style_op.clone(), *end));
                        diff.insert_value(
                            *start as usize,
                            RichtextStateChunk::new_style(style_op, AnchorType::Start),
                            (),
                        );
                    }
                    InnerListOp::StyleEnd => {
                        let (style_op, pos) = match last_style_start.take() {
                            Some((style_op, pos)) => (style_op, pos),
                            None => {
                                let Some(start_op) = oplog.get_op_that_includes(op.id().inc(-1))
                                else {
                                    panic!("Unhandled checkout case")
                                };

                                let InnerListOp::StyleStart {
                                    key,
                                    value,
                                    info,
                                    end,
                                    ..
                                } = start_op.content.as_list().unwrap()
                                else {
                                    unreachable!()
                                };
                                let style_op = Arc::new(StyleOp {
                                    lamport: op.lamport() - 1,
                                    peer: op.peer,
                                    cnt: op.id_start().counter - 1,
                                    key: key.clone(),
                                    value: value.clone(),
                                    info: *info,
                                });

                                (style_op, *end)
                            }
                        };
                        assert_eq!(style_op.peer, op.peer);
                        assert_eq!(style_op.cnt, op.id_start().counter - 1);
                        diff.insert_value(
                            pos as usize + 1,
                            RichtextStateChunk::new_style(style_op, AnchorType::End),
                            (),
                        );
                    }
                },
                _ => unreachable!(),
            },
            RichtextCalcMode::Crdt {
                tracker,
                styles,
                start_vv: _,
            } => {
                if let Some(vv) = vv {
                    tracker.checkout(vv);
                }
                match &op.raw_op().content {
                    crate::op::InnerContent::List(l) => match l {
                        InnerListOp::Insert { .. }
                        | InnerListOp::Move { .. }
                        | InnerListOp::Set { .. } => {
                            unreachable!()
                        }
                        InnerListOp::InsertText {
                            slice: _,
                            unicode_start,
                            unicode_len: len,
                            pos,
                        } => {
                            tracker.insert(
                                op.id_full(),
                                *pos as usize,
                                RichtextChunk::new_text(*unicode_start..*unicode_start + *len),
                            );
                        }
                        InnerListOp::Delete(del) => {
                            tracker.delete(
                                op.id_start(),
                                del.id_start,
                                del.start() as usize,
                                del.atom_len(),
                                del.is_reversed(),
                            );
                        }
                        InnerListOp::StyleStart {
                            start,
                            end,
                            key,
                            info,
                            value,
                        } => {
                            debug_assert!(start < end, "start: {}, end: {}", start, end);
                            let style_id = styles.len();
                            styles.push((
                                StyleOp {
                                    lamport: op.lamport(),
                                    peer: op.peer,
                                    cnt: op.id_start().counter,
                                    key: key.clone(),
                                    value: value.clone(),
                                    info: *info,
                                },
                                *end as usize,
                            ));
                            tracker.insert(
                                op.id_full(),
                                *start as usize,
                                RichtextChunk::new_style_anchor(style_id as u32, AnchorType::Start),
                            );
                        }
                        InnerListOp::StyleEnd => {
                            let id = op.id();
                            if let Some(pos) = styles.iter().rev().position(|(op, _pos)| {
                                op.peer == id.peer && op.cnt == id.counter - 1
                            }) {
                                let style_id = styles.len() - pos - 1;
                                let (_start_op, end_pos) = &styles[style_id];
                                tracker.insert(
                                    op.id_full(),
                                    // need to shift 1 because we insert the start style anchor before this pos
                                    *end_pos + 1,
                                    RichtextChunk::new_style_anchor(
                                        style_id as u32,
                                        AnchorType::End,
                                    ),
                                );
                            } else {
                                let Some(start_op) = oplog.get_op_that_includes(op.id().inc(-1))
                                else {
                                    // Checkout on richtext that export at a gc version that split
                                    // start style op and end style op apart. Won't fix for now.
                                    // It's such a rare case...
                                    unimplemented!("Unhandled checkout case")
                                };
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

                                styles.push((
                                    StyleOp {
                                        lamport: op.lamport() - 1,
                                        peer: id.peer,
                                        cnt: id.counter - 1,
                                        key: key.clone(),
                                        value: value.clone(),
                                        info: *info,
                                    },
                                    *end as usize,
                                ));
                                let style_id = styles.len() - 1;
                                tracker.insert(
                                    op.id_full(),
                                    // need to shift 1 because we insert the start style anchor before this pos
                                    *end as usize + 1,
                                    RichtextChunk::new_style_anchor(
                                        style_id as u32,
                                        AnchorType::End,
                                    ),
                                );
                            }
                        }
                    },
                    _ => unreachable!(),
                }
            }
        }
    }

    fn calculate_diff(
        &mut self,
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        _: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        match &mut *self.mode {
            RichtextCalcMode::Linear { diff, .. } => (
                InternalDiff::RichtextRaw(std::mem::take(diff)),
                DiffMode::Linear,
            ),
            RichtextCalcMode::Crdt {
                tracker, styles, ..
            } => {
                let mut delta = DeltaRope::new();
                for item in tracker.diff(info.from_vv, info.to_vv) {
                    match item {
                        CrdtRopeDelta::Retain(len) => {
                            delta.push_retain(len, ());
                        }
                        CrdtRopeDelta::Insert {
                            chunk: value,
                            id,
                            lamport,
                        } => match value.value() {
                            RichtextChunkValue::Text(text) => {
                                delta.push_insert(
                                    RichtextStateChunk::Text(
                                        // PERF: can be speedup by acquiring lock on arena
                                        TextChunk::new(
                                            oplog.arena.slice_by_unicode(
                                                text.start as usize..text.end as usize,
                                            ),
                                            IdFull::new(id.peer, id.counter, lamport.unwrap()),
                                        ),
                                    ),
                                    (),
                                );
                            }
                            RichtextChunkValue::StyleAnchor { id, anchor_type } => {
                                delta.push_insert(
                                    RichtextStateChunk::Style {
                                        style: Arc::new(styles[id as usize].0.clone()),
                                        anchor_type,
                                    },
                                    (),
                                );
                            }
                            RichtextChunkValue::Unknown(len) => {
                                // assert not unknown id
                                assert_ne!(id.peer, PeerID::MAX);
                                let mut id = id;
                                let mut acc_len = 0;
                                let end = id.counter + len as Counter;
                                let shallow_root =
                                    oplog.shallow_since_vv().get(&id.peer).copied().unwrap_or(0);
                                if id.counter < shallow_root {
                                    // need to find the content between id.counter ~ target_end in gc state
                                    let target_end = shallow_root.min(end);
                                    oplog.with_history_cache(|h| {
                                        let chunks = h.find_text_chunks_in(
                                            idx,
                                            IdSpan::new(id.peer, id.counter, target_end),
                                        );
                                        for c in chunks {
                                            acc_len += c.rle_len();
                                            delta.push_insert(c, ());
                                        }
                                    });
                                    id.counter = shallow_root;
                                }

                                if id.counter < end {
                                    for rich_op in
                                        oplog.iter_ops(IdSpan::new(id.peer, id.counter, end))
                                    {
                                        acc_len += rich_op.content_len();
                                        let op = rich_op.op();
                                        let lamport = rich_op.lamport();
                                        let content = op.content.as_list().unwrap();
                                        match content {
                                            InnerListOp::InsertText { slice, .. } => {
                                                delta.push_insert(
                                                    RichtextStateChunk::Text(TextChunk::new(
                                                        slice.clone(),
                                                        IdFull::new(id.peer, op.counter, lamport),
                                                    )),
                                                    (),
                                                );
                                            }
                                            _ => unreachable!("{:?}", content),
                                        }
                                    }
                                }

                                debug_assert_eq!(acc_len, len as usize);
                            }
                            RichtextChunkValue::MoveAnchor => unreachable!(),
                        },
                        CrdtRopeDelta::Delete(len) => {
                            delta.push_delete(len);
                        }
                    }
                }

                (InternalDiff::RichtextRaw(delta), DiffMode::Checkout)
            }
        }
    }

    fn finish_this_round(&mut self) {
        match &mut *self.mode {
            RichtextCalcMode::Crdt { .. } => {}
            RichtextCalcMode::Linear {
                diff,
                last_style_start,
            } => {
                *diff = DeltaRope::new();
                last_style_start.take();
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct MovableListDiffCalculator {
    list: Box<ListDiffCalculator>,
    inner: Box<MovableListInner>,
}

#[derive(Debug)]
struct MovableListInner {
    changed_elements: FxHashMap<CompactIdLp, ElementDelta>,
    move_id_to_elem_id: FxHashMap<ID, IdLp>,
    current_mode: DiffMode,
}

impl DiffCalculatorTrait for MovableListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector, mode: DiffMode) {
        if !vv.includes_vv(&self.list.start_vv) || !self.list.tracker.all_vv().includes_vv(vv) {
            *self.list.tracker = RichtextTracker::new_with_unknown();
            self.list.start_vv = vv.clone();
        }

        self.list.tracker.checkout(vv);
        self.inner.current_mode = mode;
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

        // If it's checkout mode, we don't need to track the changes
        // we only need the element ids
        match l {
            InnerListOp::Insert { slice, pos: _ } => {
                let op_id = op.id_full().idlp();
                for i in 0..slice.atom_len() {
                    let id = op_id.inc(i as Counter);
                    let value = oplog.arena.get_value(slice.0.start as usize + i).unwrap();

                    self.inner.changed_elements.insert(
                        id.compact(),
                        ElementDelta {
                            pos: Some(id),
                            value: value.clone(),
                            value_updated: true,
                            value_id: Some(id),
                        },
                    );
                }
            }
            InnerListOp::Delete(_) => {}
            InnerListOp::Move { elem_id, .. } => {
                let idlp = IdLp::new(op.peer, op.lamport());
                match self.inner.changed_elements.get_mut(&elem_id.compact()) {
                    Some(change) => {
                        if change.pos.is_some() && change.pos.as_ref().unwrap() > &idlp {
                        } else {
                            change.pos = Some(idlp);
                        }
                    }
                    None => {
                        self.inner.changed_elements.insert(
                            elem_id.compact(),
                            ElementDelta {
                                pos: Some(idlp),
                                value: LoroValue::Null,
                                value_updated: false,
                                value_id: None,
                            },
                        );
                    }
                }
            }
            InnerListOp::Set { elem_id, value } => {
                let idlp = IdLp::new(op.peer, op.lamport());
                match self.inner.changed_elements.get_mut(&elem_id.compact()) {
                    Some(change) => {
                        if change.value_id.is_some() && change.value_id.as_ref().unwrap() > &idlp {
                        } else {
                            change.value_id = Some(idlp);
                            change.value = value.clone();
                        }
                    }
                    None => {
                        self.inner.changed_elements.insert(
                            elem_id.compact(),
                            ElementDelta {
                                pos: None,
                                value: value.clone(),
                                value_updated: true,
                                value_id: Some(idlp),
                            },
                        );
                    }
                }
            }

            InnerListOp::StyleStart { .. } => unreachable!(),
            InnerListOp::StyleEnd => unreachable!(),
            InnerListOp::InsertText { .. } => unreachable!(),
        }

        let is_checkout = matches!(self.inner.current_mode, DiffMode::Checkout);

        {
            // Apply change on the list items
            let this = &mut self.list;
            if let Some(vv) = vv {
                this.tracker.checkout(vv);
            }

            let real_op = op.op();
            match &real_op.content {
                crate::op::InnerContent::List(l) => match l {
                    InnerListOp::Insert { slice, pos } => {
                        this.tracker.insert(
                            op.id_full(),
                            *pos,
                            RichtextChunk::new_text(slice.0.clone()),
                        );
                    }
                    InnerListOp::Delete(del) => {
                        this.tracker.delete(
                            op.id_start(),
                            del.id_start,
                            del.start() as usize,
                            del.atom_len(),
                            del.is_reversed(),
                        );
                    }
                    InnerListOp::Move { from, elem_id, to } => {
                        self.inner.move_id_to_elem_id.insert(op.id(), *elem_id);
                        if !this.tracker.current_vv().includes_id(op.id()) {
                            let last_pos = if is_checkout {
                                // TODO: PERF: this lookup can be optimized
                                oplog.with_history_cache(|h| {
                                    let list = &h.get_checkout_index().movable_list;
                                    list.last_pos(
                                        *elem_id,
                                        this.tracker.current_vv(),
                                        // TODO: PERF: Provide the lamport of to version
                                        Lamport::MAX,
                                        oplog,
                                    )
                                    .unwrap()
                                    .id()
                                })
                            } else {
                                // When it's import or linear mode, we need to use a fake id
                                // because we want to avoid using the history cache
                                //
                                // This ID will not be used. Because it will only be used when
                                // we switch to an older version. And we know it's for importing and
                                // to version is always after from version (!is_checkout), so that
                                // we don't need to checkout to the version before from.
                                const FAKE_ID: ID = ID {
                                    peer: PeerID::MAX - 2,
                                    counter: 0,
                                };
                                FAKE_ID
                            };
                            this.tracker.move_item(
                                op.id_full(),
                                last_pos,
                                *from as usize,
                                *to as usize,
                            );
                        }
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
        self.list.finish_this_round();
    }

    #[instrument(skip(self, oplog, on_new_container))]
    fn calculate_diff(
        &mut self,
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        let (InternalDiff::ListRaw(list_diff), diff_mode) =
            self.list.calculate_diff(idx, oplog, info, |_| {})
        else {
            unreachable!()
        };

        assert_eq!(diff_mode, DiffMode::Checkout);
        let is_checkout = matches!(
            self.inner.current_mode,
            DiffMode::Checkout | DiffMode::Import
        );
        let mut element_changes: FxHashMap<CompactIdLp, ElementDelta> = if is_checkout {
            FxHashMap::default()
        } else {
            std::mem::take(&mut self.inner.changed_elements)
        };

        if is_checkout {
            for id in self.inner.changed_elements.keys() {
                element_changes.insert(*id, ElementDelta::placeholder());
            }
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
                        let len = insert.length();
                        let id = insert.id;
                        let mut new_insert = SmallVec::with_capacity(len);
                        for i in 0..len {
                            let id = id.inc(i as i32);
                            let elem_id =
                                if let Some(e) = self.inner.move_id_to_elem_id.get(&id.id()) {
                                    e.compact()
                                } else {
                                    insert.elem_id.unwrap_or_else(|| id.idlp().compact())
                                };
                            if is_checkout {
                                // add the related element id
                                element_changes.insert(elem_id, ElementDelta::placeholder());
                            }
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

        if is_checkout {
            oplog.with_history_cache(|history_cache| {
                let checkout_index = &history_cache.get_checkout_index().movable_list;
                element_changes.retain(|id, change| {
                    let id = id.to_id();
                    // It can be None if the target does not exist before the `to` version
                    // But we don't need to calc from, because the deletion is handled by the diff from list items

                    // TODO: PERF: Provide the lamport of to version
                    let Some(pos) = checkout_index.last_pos(id, info.to_vv, Lamport::MAX, oplog)
                    else {
                        return false;
                    };
                    // TODO: PERF: Provide the lamport of to version
                    let value = checkout_index
                        .last_value(id, info.to_vv, Lamport::MAX, oplog)
                        .unwrap();
                    // TODO: PERF: Provide the lamport of to version
                    let old_pos = checkout_index.last_pos(id, info.from_vv, Lamport::MAX, oplog);
                    // TODO: PERF: Provide the lamport of to version
                    let old_value =
                        checkout_index.last_value(id, info.from_vv, Lamport::MAX, oplog);
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
            });
        }

        let diff = MovableListInnerDelta {
            list: list_diff,
            elements: element_changes,
        };

        (InternalDiff::MovableList(diff), self.inner.current_mode)
    }
}

impl MovableListDiffCalculator {
    fn new(_container: ContainerIdx) -> MovableListDiffCalculator {
        MovableListDiffCalculator {
            list: Default::default(),
            inner: Box::new(MovableListInner {
                changed_elements: Default::default(),
                current_mode: DiffMode::Checkout,
                move_id_to_elem_id: Default::default(),
            }),
        }
    }
}

#[test]
fn test_size() {
    let text = RichtextDiffCalculator::new();
    let size = std::mem::size_of_val(&text);
    assert!(size < 50, "RichtextDiffCalculator size: {}", size);
    let list = MovableListDiffCalculator::new(ContainerIdx::from_index_and_type(
        0,
        loro_common::ContainerType::MovableList,
    ));
    let size = std::mem::size_of_val(&list);
    assert!(size < 50, "MovableListDiffCalculator size: {}", size);
    let calc = ContainerDiffCalculator::Richtext(text);
    let size = std::mem::size_of_val(&calc);
    assert!(size < 50, "ContainerDiffCalculator size: {}", size);
}

/// The crux differential for the O(log n) seam-rank readout: on one settled
/// per-container tracker, [`RichtextDiffCalculator::id_to_activated_index`] must
/// return exactly what the O(n) oracle
/// (`get_target_id_latest_index_at_new_version`, reached via `get_id_latest_pos`)
/// returns, for both live and tombstoned ids. This pins the prefix-sum and the
/// offset-basis equivalence that make the fast path byte-exact. Full-history
/// only: on a shallow doc the two readouts diverge in resolvability, not rank.
#[cfg(all(test, feature = "persistent-anchor-tracker"))]
mod readout_differential {
    use crate::{cursor::Side, handler::HandlerTrait, LoroDoc};
    use loro_common::ID;

    /// Tiny deterministic xorshift PRNG so the corpus is reproducible.
    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    fn doc(peer: u64) -> LoroDoc {
        let doc = LoroDoc::default();
        doc.set_peer_id(peer).unwrap();
        doc.start_auto_commit();
        doc
    }

    fn sync(a: &LoroDoc, b: &LoroDoc) {
        a.import(
            &b.export(crate::encoding::ExportMode::all_updates())
                .unwrap(),
        )
        .unwrap();
        b.import(
            &a.export(crate::encoding::ExportMode::all_updates())
                .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn id_to_activated_index_matches_oracle_on_settled_tracker() {
        let mut rng = Rng(0x00C0_FFEE_1357_9BDF);
        let a = doc(1);
        let b = doc(2);
        let docs = [&a, &b];

        a.get_text("text")
            .insert_unicode(0, "seed-content")
            .unwrap();
        sync(&a, &b);

        let mut captured: Vec<ID> = Vec::new();
        for round in 0..80 {
            let d = docs[rng.below(docs.len())];
            let text = d.get_text("text");
            let len = text.len_unicode();

            if len == 0 || rng.below(3) != 0 {
                let pos = if len == 0 { 0 } else { rng.below(len + 1) };
                let ch = (b'a' + (round % 26) as u8) as char;
                text.insert_unicode(pos, &ch.to_string()).unwrap();
                if let Some(id) = text.get_cursor(pos, Side::Middle).and_then(|c| c.id) {
                    captured.push(id);
                }
            } else {
                let start = rng.below(len);
                let del = 1 + rng.below((len - start).min(3));
                // Capture ids that are about to be tombstoned.
                for i in start..(start + del) {
                    if let Some(id) = text.get_cursor(i, Side::Middle).and_then(|c| c.id) {
                        captured.push(id);
                    }
                }
                text.delete_unicode(start, del).unwrap();
            }

            if rng.below(2) == 0 {
                sync(&a, &b);
            }
        }

        // Full merge so both docs hold the same history, then flush A's pending
        // transaction so the oplog head reflects every op.
        sync(&a, &b);
        sync(&a, &b);

        captured.sort();
        captured.dedup();
        assert!(captured.len() >= 10, "expected a decent pool of ids");

        // Build a tracker straight from genesis, settled at head, then read both
        // the O(log n) index and the O(n) oracle off that very same tracker.
        let idx = a.get_text("text").idx();
        let oplog = a.oplog().lock();
        let cached = super::CachedTextTracker::build(&oplog, idx);

        let mut saw_live = false;
        let mut saw_tombstoned = false;
        for id in &captured {
            let fast = cached.id_to_activated_index(*id);
            let oracle = cached.calc.get_id_latest_pos(*id);
            assert_eq!(
                fast, oracle,
                "readout diverged from the O(n) oracle for {id:?}"
            );
            match fast.map(|p| p.side) {
                Some(Side::Middle) => saw_live = true,
                Some(Side::Left) => saw_tombstoned = true,
                _ => {}
            }
        }
        assert!(
            saw_live && saw_tombstoned,
            "corpus must exercise both live and tombstoned ids (live: {saw_live}, tomb: {saw_tombstoned})"
        );
    }

    #[test]
    fn id_to_activated_index_tracks_within_leaf_offset() {
        // A multi-char run inserted in a SINGLE op shares one rope leaf, so the
        // interior chars sit at within-leaf offset > 0. This is the only shape
        // that pins the offset term of the readout: dropping the
        // `ThisElemAndOffset` contribution (or double-counting it in the return)
        // shifts every interior id and diverges from the O(n) oracle. The fuzz
        // corpus inserts one char at a time, so every id there is at offset 0 and
        // an offset regression stays green — this test is what turns it red.
        let a = doc(1);
        a.get_text("text").insert_unicode(0, "hello").unwrap();
        // Flush the auto-commit transaction so the oplog head holds the op.
        a.commit_then_renew();

        let text = a.get_text("text");
        let idx = text.idx();
        // The id of each live char; interior chars (index > 0) have offset > 0.
        let ids: Vec<ID> = (0..5)
            .map(|i| {
                text.get_cursor(i, Side::Middle)
                    .and_then(|c| c.id)
                    .expect("live char must have an id")
            })
            .collect();

        let oplog = a.oplog().lock();
        let cached = super::CachedTextTracker::build(&oplog, idx);

        let mut saw_offset = false;
        for (expected_pos, id) in ids.iter().enumerate() {
            let fast = cached.id_to_activated_index(*id);
            let oracle = cached.calc.get_id_latest_pos(*id);
            assert_eq!(fast, oracle, "readout diverged from the oracle for {id:?}");
            // A live char's activated index equals its insertion position; the
            // interior ones exercise the within-leaf offset explicitly.
            assert_eq!(
                fast.as_ref().map(|p| p.pos),
                Some(expected_pos),
                "live char at {expected_pos} resolved to the wrong index"
            );
            assert_eq!(fast.as_ref().map(|p| p.side), Some(Side::Middle));
            if expected_pos > 0 {
                saw_offset = true;
            }
        }
        assert!(saw_offset, "must query interior chars at offset > 0");
    }
}
