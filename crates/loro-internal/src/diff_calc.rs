use std::sync::Arc;

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{HasIdSpan, PeerID, ID};

use crate::{
    change::Lamport,
    container::{
        idx::ContainerIdx,
        richtext::{
            richtext_state::RichtextStateChunk, AnchorType, CrdtRopeDelta, RichtextChunk,
            RichtextChunkValue, RichtextTracker, StyleOp,
        },
    },
    delta::{Delta, MapDelta, MapValue},
    event::Diff,
    id::Counter,
    op::RichOp,
    span::{HasId, HasLamport},
    text::tracker::Tracker,
    version::Frontiers,
    InternalString, VersionVector,
};

use super::{event::InternalContainerDiff, oplog::OpLog};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
///
/// TODO: persist diffCalculator and skip processed version
#[derive(Debug, Default)]
pub struct DiffCalculator {
    calculators: FxHashMap<ContainerIdx, ContainerDiffCalculator>,
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
            let mut after = after;
            let mut before = before;
            let mut merged = before.clone();
            let mut before_frontiers = before_frontiers;
            let mut after_frontiers = after_frontiers;
            merged.merge(after);
            let empty_vv: VersionVector = Default::default();
            if !after.includes_vv(before) {
                // If after is not after before, we need to calculate the diff from the beginning
                //
                // This is required because of [MapDiffCalculator]. It can be removed with
                // a better data structure. See #114.
                before = &empty_vv;
                after = &merged;
                before_frontiers = None;
                after_frontiers = None;
                self.has_all = true;
                self.last_vv = Default::default();
            } else if before.is_empty() {
                self.has_all = true;
                self.last_vv = Default::default();
            }

            let (lca, iter) =
                oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);

            let mut started_set = FxHashSet::default();
            for (change, vv) in iter {
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

                let mut visited = FxHashSet::default();
                for op in change.ops.iter() {
                    let calculator =
                        self.calculators.entry(op.container).or_insert_with(|| {
                            match op.container.get_type() {
                                crate::ContainerType::Text => {
                                    ContainerDiffCalculator::Text(TextDiffCalculator::default())
                                }
                                crate::ContainerType::Richtext => {
                                    ContainerDiffCalculator::Richtext(
                                        RichtextDiffCalculator::default(),
                                    )
                                }
                                crate::ContainerType::Map => {
                                    ContainerDiffCalculator::Map(MapDiffCalculator::new())
                                }
                                crate::ContainerType::List => {
                                    ContainerDiffCalculator::List(ListDiffCalculator::default())
                                }
                            }
                        });

                    if !started_set.contains(&op.container) {
                        started_set.insert(op.container);
                        calculator.start_tracking(oplog, &lca);
                    }

                    if visited.contains(&op.container) {
                        // don't checkout if we have already checked out this container in this round
                        calculator.apply_change(oplog, RichOp::new_by_change(change, op), None);
                    } else {
                        calculator.apply_change(
                            oplog,
                            RichOp::new_by_change(change, op),
                            Some(&vv.borrow()),
                        );
                        visited.insert(op.container);
                    }
                }
            }

            for (_, calculator) in self.calculators.iter_mut() {
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

        let mut diffs = Vec::with_capacity(self.calculators.len());
        if let Some(set) = affected_set {
            // only visit the affected containers
            for idx in set {
                let calc = self.calculators.get_mut(&idx).unwrap();
                diffs.push(InternalContainerDiff {
                    idx,
                    diff: calc.calculate_diff(oplog, before, after),
                });
            }
        } else {
            for (&idx, calculator) in self.calculators.iter_mut() {
                diffs.push(InternalContainerDiff {
                    idx,
                    diff: calculator.calculate_diff(oplog, before, after),
                });
            }
        }

        diffs
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
pub trait DiffCalculatorTrait {
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
    ) -> Diff;
}

#[enum_dispatch(DiffCalculatorTrait)]
#[derive(Debug)]
enum ContainerDiffCalculator {
    Text(TextDiffCalculator),
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
    Richtext(RichtextDiffCalculator),
}

#[derive(Default)]
struct TextDiffCalculator {
    tracker: Tracker,
}

impl std::fmt::Debug for TextDiffCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextDiffCalculator")
            // .field("tracker", &self.tracker)
            .finish()
    }
}

#[derive(Debug, Default)]
struct MapDiffCalculator {
    grouped: FxHashMap<InternalString, CompactRegister>,
}

impl MapDiffCalculator {
    pub(crate) fn new() -> Self {
        Self {
            grouped: Default::default(),
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
        let map = op.op().content.as_map().unwrap();
        self.grouped
            .entry(map.key.clone())
            .or_default()
            .push(CompactMapValue {
                lamport: op.lamport(),
                peer: op.client_id(),
                counter: op.id_start().counter,
                value: op.op().content.as_map().unwrap().value,
            });
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        let mut changed = Vec::new();
        for (k, g) in self.grouped.iter_mut() {
            let (peek_from, peek_to) = g.peek_at_ab(from, to);
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
                    let value = v.value.and_then(|v| oplog.arena.get_value(v as usize));
                    MapValue {
                        counter: v.counter,
                        value,
                        lamport: (v.lamport, v.peer),
                    }
                })
                .unwrap_or_else(|| MapValue {
                    counter: 0,
                    value: None,
                    lamport: (0, 0),
                });
            updated.insert(key, value);
        }

        Diff::NewMap(MapDelta { updated })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CompactMapValue {
    lamport: Lamport,
    peer: PeerID,
    counter: Counter,
    value: Option<u32>,
}

impl HasId for CompactMapValue {
    fn id_start(&self) -> ID {
        ID::new(self.peer, self.counter)
    }
}

use compact_register::CompactRegister;
use rle::HasLength;

mod compact_register {
    use std::collections::BTreeSet;

    use super::*;
    #[derive(Debug, Default)]
    pub(super) struct CompactRegister {
        tree: BTreeSet<CompactMapValue>,
    }

    impl CompactRegister {
        pub fn push(&mut self, value: CompactMapValue) {
            self.tree.insert(value);
        }

        pub fn peek_at_ab(
            &self,
            a: &VersionVector,
            b: &VersionVector,
        ) -> (Option<CompactMapValue>, Option<CompactMapValue>) {
            let mut max_a: Option<CompactMapValue> = None;
            let mut max_b: Option<CompactMapValue> = None;
            for v in self.tree.iter().rev() {
                if b.get(&v.peer).copied().unwrap_or(0) > v.counter {
                    max_b = Some(*v);
                    break;
                }
            }

            for v in self.tree.iter().rev() {
                if a.get(&v.peer).copied().unwrap_or(0) > v.counter {
                    max_a = Some(*v);
                    break;
                }
            }

            (max_a, max_b)
        }
    }
}

#[derive(Default)]
struct ListDiffCalculator {
    tracker: Tracker,
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
        if !vv.includes_vv(self.tracker.start_vv()) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Tracker::new(vv.clone(), Counter::MAX / 2);
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
        self.tracker.track_apply(&op);
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        Diff::SeqRaw(self.tracker.diff(from, to))
    }
}

impl DiffCalculatorTrait for TextDiffCalculator {
    fn start_tracking(&mut self, _oplog: &super::oplog::OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(self.tracker.start_vv()) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Tracker::new(vv.clone(), Counter::MAX / 2);
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

        self.tracker.track_apply(&op);
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        Diff::SeqRaw(self.tracker.diff(from, to))
    }
}

#[derive(Debug, Default)]
struct RichtextDiffCalculator {
    start_vv: VersionVector,
    tracker: RichtextTracker,
    styles: Vec<StyleOp>,
}

impl DiffCalculatorTrait for RichtextDiffCalculator {
    fn start_tracking(&mut self, _oplog: &super::oplog::OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = RichtextTracker::new_with_unknown();
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

        match &op.op().content {
            crate::op::InnerContent::List(l) => match l {
                crate::container::list::list_op::InnerListOp::Insert { slice, pos } => {
                    self.tracker.insert(
                        op.id_start(),
                        *pos,
                        RichtextChunk::new_text(slice.0.clone()),
                    );
                }
                crate::container::list::list_op::InnerListOp::Delete(del) => {
                    self.tracker.delete(
                        op.id_start(),
                        del.start() as usize,
                        del.atom_len(),
                        del.pos < 0,
                    );
                }
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    info,
                } => {
                    debug_assert!(start < end, "start: {}, end: {}", start, end);
                    let style_id = self.styles.len();
                    self.styles.push(StyleOp {
                        lamport: op.lamport(),
                        peer: op.peer,
                        cnt: op.id_start().counter,
                        key: key.clone(),
                        info: *info,
                    });
                    self.tracker.insert(
                        op.id_start(),
                        *start as usize,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::Start),
                    );
                    self.tracker.insert(
                        op.id_start().inc(1),
                        // need to shift 1 because we insert the start style anchor before this pos
                        *end as usize + 1,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::End),
                    );
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => {}
            },
            crate::op::InnerContent::Map(_) => unreachable!(),
        }
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        let mut delta = Delta::new();
        for item in self.tracker.diff(from, to) {
            match item {
                CrdtRopeDelta::Retain(len) => {
                    delta = delta.retain(len);
                }
                CrdtRopeDelta::Insert(value) => match value.value() {
                    RichtextChunkValue::Text(text) => {
                        delta = delta.insert(RichtextStateChunk::Text {
                            unicode_len: text.len() as i32,
                            // PERF: can be speedup by acquiring lock on arena
                            text: oplog
                                .arena
                                .slice_by_unicode(text.start as usize..text.end as usize),
                        });
                    }
                    RichtextChunkValue::StyleAnchor { id, anchor_type } => {
                        delta = delta.insert(RichtextStateChunk::Style {
                            style: Arc::new(self.styles[id as usize].clone()),
                            anchor_type,
                        });
                    }
                    RichtextChunkValue::Unknown(_) => unreachable!(),
                },
                CrdtRopeDelta::Delete(len) => {
                    delta = delta.delete(len);
                }
            }
        }

        debug_log::debug_dbg!(&delta, from, to);
        debug_log::debug_dbg!(&self.tracker);
        Diff::RichtextRaw(delta)
    }
}
