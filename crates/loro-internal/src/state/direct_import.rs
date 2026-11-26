//! Apply a causal append straight to [`DocState`], skipping diff calculation.
//!
//! This is only for a linear extension whose stored positions are the document
//! positions: scalar/text ops on live plain text, scalar lists, scalar maps, and
//! counters. Styled text, trees, movable lists, child containers, binary values,
//! and deleted/dangling containers return `Ok(false)` so the diff calculator
//! keeps their semantics. A `false` return writes nothing. An `Err` is also
//! before any container write, so the caller only has to roll the oplog back.

use std::borrow::Cow;

use loro_common::{ContainerType, Counter, HasCounterSpan, LoroError, LoroResult, LoroValue, ID};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    change::Lamport,
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpanWithId, InnerListOp, ListOp},
        map::MapSet,
    },
    dag::{DagUtils, MeetAsBase},
    diff_calc::DiffMode,
    op::{InnerContent, ListSlice, Op, RawOp, RawOpContent},
    oplog::{BlockChangeRef, OpLog},
    version::{Frontiers, VersionVector},
};

#[cfg(feature = "counter")]
use crate::op::FutureInnerContent;

use super::{DocState, State};

const MAX_CHAIN_CHANGES: usize = 100_000;

struct Step {
    change: BlockChangeRef,
    /// First counter of this change that is not already in `old_vv`.
    from_counter: Counter,
}

/// `Ok(true)` means the state now matches the oplog. `Ok(false)` means nothing
/// was written and the caller should use the diff calculator.
pub(crate) fn try_apply(
    oplog: &OpLog,
    state: &mut DocState,
    old_frontiers: &Frontiers,
    old_vv: &VersionVector,
    rollback_enabled: bool,
    origin: &str,
) -> LoroResult<bool> {
    if oplog.uncommitted_change.is_some() || oplog.batch_importing {
        return Ok(false);
    }
    if state.frontiers != *old_frontiers || !oplog.shallow_since_vv().is_empty() {
        return Ok(false);
    }
    // A missing block is an oplog error. Fail before any container write.
    oplog.check_history_parsable()?;
    let new_frontiers = oplog.frontiers();
    if new_frontiers.as_single().is_none() {
        return Ok(false);
    }
    let (meet, mode) = oplog
        .dag()
        .try_find_meet_and_mode(old_frontiers, new_frontiers)?;
    match meet {
        MeetAsBase::Valid(base) if base == *old_frontiers && mode == DiffMode::Linear => {}
        _ => return Ok(false),
    }

    let Some(steps) = collect_steps(oplog, old_frontiers, old_vv) else {
        return Ok(false);
    };
    if !validate_and_simulate(state, &steps)? {
        return Ok(false);
    }
    if super::take_import_apply_failpoint(origin) {
        return Err(LoroError::internal("state apply failpoint"));
    }
    if rollback_enabled {
        state.store.begin_created_journal();
    }
    state.advance_access_epoch();
    apply_ops(state, &steps);
    state.frontiers = oplog.frontiers().clone();
    state.alive_containers_cache = None;
    Ok(true)
}

fn collect_steps(
    oplog: &OpLog,
    old_frontiers: &Frontiers,
    old_vv: &VersionVector,
) -> Option<Vec<Step>> {
    let mut cursor = oplog.frontiers().as_single()?;
    let mut seen = FxHashSet::default();
    let mut steps = Vec::new();
    let mut finished = false;
    for _ in 0..MAX_CHAIN_CHANGES {
        let change = oplog.get_change_at(cursor)?;
        if !seen.insert(change.id) {
            return None;
        }
        let old_end = old_vv.get(&change.id.peer).copied().unwrap_or(0);
        if change.ctr_end() <= old_end {
            return None;
        }
        if change.id.counter < old_end {
            steps.push(Step {
                change,
                from_counter: old_end,
            });
            finished = true;
            break;
        }
        let change_id = change.id;
        let deps = change.deps().clone();
        steps.push(Step {
            change,
            from_counter: change_id.counter,
        });
        if deps == *old_frontiers {
            finished = true;
            break;
        }
        if deps.len() == 1 {
            let dep = deps.as_single()?;
            if old_vv.includes_id(dep) {
                return None;
            }
            cursor = dep;
            continue;
        }
        let mut next = None;
        let mut extra = false;
        for dep in deps.iter() {
            if old_vv.includes_id(dep) {
                continue;
            }
            if next.is_some() {
                extra = true;
                break;
            }
            next = Some(dep);
        }
        if extra || next.is_none() {
            return None;
        }
        cursor = next?;
    }
    if !finished {
        return None;
    }
    steps.reverse();
    Some(steps)
}

fn try_visit_ops(
    steps: &[Step],
    mut f: impl FnMut(&BlockChangeRef, &Op) -> LoroResult<bool>,
) -> LoroResult<bool> {
    for step in steps {
        for op in step.change.ops().iter() {
            let op_len = rle::HasLength::atom_len(op);
            let op_end = op.counter.saturating_add(op_len as Counter);
            if op_end <= step.from_counter {
                continue;
            }
            if op.counter < step.from_counter {
                let sliced =
                    rle::Sliceable::slice(op, (step.from_counter - op.counter) as usize, op_len);
                if !f(&step.change, &sliced)? {
                    return Ok(false);
                }
            } else if !f(&step.change, op)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn is_direct_op(op: &Op) -> bool {
    match op.container.get_type() {
        ContainerType::Text => matches!(
            &op.content,
            InnerContent::List(InnerListOp::InsertText { .. } | InnerListOp::Delete(_))
        ),
        ContainerType::List => matches!(
            &op.content,
            InnerContent::List(InnerListOp::Insert { .. } | InnerListOp::Delete(_))
        ),
        ContainerType::Map => matches!(&op.content, InnerContent::Map(_)),
        #[cfg(feature = "counter")]
        ContainerType::Counter => {
            matches!(
                &op.content,
                InnerContent::Future(FutureInnerContent::Counter(_))
            )
        }
        _ => false,
    }
}

fn container_eligible(state: &mut DocState, idx: ContainerIdx) -> bool {
    let Some(cid) = state.arena.idx_to_id(idx) else {
        return false;
    };
    if cid.is_mergeable() || cid.is_unknown() {
        return false;
    }
    if !cid.is_root() && state.arena.get_depth(idx).is_none() {
        return false;
    }
    if state.is_deleted(idx) {
        return false;
    }
    if idx.get_type() == ContainerType::Text {
        if let Some(State::RichtextState(text)) = state.store.get_container(idx) {
            if text.has_styles() {
                return false;
            }
        }
    }
    true
}

fn validate_and_simulate(state: &mut DocState, steps: &[Step]) -> LoroResult<bool> {
    let mut seen_containers: FxHashSet<ContainerIdx> = FxHashSet::default();
    let mut empty_lazy_texts: FxHashSet<ContainerIdx> = FxHashSet::default();
    let mut text_len: FxHashMap<ContainerIdx, usize> = FxHashMap::default();
    let mut list_len: FxHashMap<ContainerIdx, usize> = FxHashMap::default();

    try_visit_ops(steps, |_, op| {
        if !is_direct_op(op) {
            return Ok(false);
        }
        if seen_containers.insert(op.container) {
            if !container_eligible(state, op.container) {
                return Ok(false);
            }
            if op.container.get_type() == ContainerType::Text {
                let started_empty_lazy = match state.store.get_container(op.container) {
                    None => true,
                    Some(State::RichtextState(t)) => t.is_empty_lazy_loader(),
                    _ => false,
                };
                if started_empty_lazy {
                    empty_lazy_texts.insert(op.container);
                }
            }
        }

        match &op.content {
            InnerContent::List(InnerListOp::InsertText {
                slice,
                unicode_len,
                pos,
            }) => {
                if std::str::from_utf8(slice.as_bytes()).is_err() {
                    return Err(LoroError::DecodeError(
                        "text insert is not utf-8".to_string().into(),
                    ));
                }
                let len = projected_len(&mut text_len, state, op.container, current_text_len);
                let pos = *pos as usize;
                if pos > *len {
                    return Err(LoroError::DecodeError(
                        format!("text diff retains {pos} items but state only has {len}")
                            .into_boxed_str(),
                    ));
                }
                if pos < *len && empty_lazy_texts.contains(&op.container) {
                    return Ok(false);
                }
                *len += *unicode_len as usize;
            }
            InnerContent::List(InnerListOp::Insert { slice, pos }) => {
                if slice.as_slice().iter().any(|v| v.is_container()) {
                    return Ok(false);
                }
                let len = projected_len(&mut list_len, state, op.container, current_list_len);
                let pos = *pos as usize;
                if pos > *len {
                    return Err(LoroError::internal(format!(
                        "list diff retains {pos} items but state only has {len}",
                    )));
                }
                *len += slice.len();
            }
            InnerContent::List(InnerListOp::Delete(del)) => {
                let start = del.start();
                let n = rle::HasLength::atom_len(del);
                if start < 0 || n == 0 {
                    bounds_err(op.container.get_type(), 0, 0)?;
                }
                let start = start as usize;
                if op.container.get_type() == ContainerType::Text {
                    let len = projected_len(&mut text_len, state, op.container, current_text_len);
                    if start > *len {
                        return Err(LoroError::DecodeError(
                            format!("text diff retains {start} items but state only has {len}")
                                .into_boxed_str(),
                        ));
                    }
                    if start.saturating_add(n) > *len {
                        return Err(LoroError::DecodeError(
                            format!("text diff deletes {n} at {start} but state only has {len}")
                                .into_boxed_str(),
                        ));
                    }
                    if empty_lazy_texts.contains(&op.container) {
                        return Ok(false);
                    }
                    *len -= n;
                } else {
                    if let Some(State::ListState(list)) = state.store.get_container(op.container) {
                        if list.has_child_containers() {
                            return Ok(false);
                        }
                    }
                    let len = projected_len(&mut list_len, state, op.container, current_list_len);
                    if start > *len {
                        return Err(LoroError::internal(format!(
                            "list diff retains {start} items but state only has {len}",
                        )));
                    }
                    if start.saturating_add(n) > *len {
                        return Err(LoroError::internal(format!(
                            "list diff deletes {n} at {start} but state only has {len}",
                        )));
                    }
                    *len -= n;
                }
            }
            InnerContent::Map(set) => {
                if matches!(
                    &set.value,
                    Some(LoroValue::Container(_) | LoroValue::Binary(_))
                ) {
                    return Ok(false);
                }
                if let Some(State::MapState(map)) = state.store.get_container(op.container) {
                    if matches!(
                        map.get(set.key.as_ref()),
                        Some(LoroValue::Container(_) | LoroValue::Binary(_))
                    ) {
                        return Ok(false);
                    }
                }
            }
            #[cfg(feature = "counter")]
            InnerContent::Future(FutureInnerContent::Counter(_)) => {}
            _ => unreachable!("is_direct_op only keeps direct ops"),
        }
        Ok(true)
    })
}

fn bounds_err(kind: ContainerType, pos: usize, len: usize) -> LoroResult<()> {
    if kind == ContainerType::Text {
        Err(LoroError::DecodeError(
            format!("text diff deletes at {pos} but state only has {len}").into_boxed_str(),
        ))
    } else {
        Err(LoroError::internal(format!(
            "list diff deletes at {pos} but state only has {len}",
        )))
    }
}

fn projected_len<'a>(
    lens: &'a mut FxHashMap<ContainerIdx, usize>,
    state: &mut DocState,
    idx: ContainerIdx,
    current: impl Fn(&mut DocState, ContainerIdx) -> usize,
) -> &'a mut usize {
    lens.entry(idx).or_insert_with(|| current(state, idx))
}

fn current_text_len(state: &mut DocState, idx: ContainerIdx) -> usize {
    match state.store.get_container(idx) {
        Some(State::RichtextState(text)) => text.len_entity(),
        _ => 0,
    }
}

fn current_list_len(state: &mut DocState, idx: ContainerIdx) -> usize {
    match state.store.get_container(idx) {
        Some(State::ListState(list)) => list.len(),
        _ => 0,
    }
}

fn apply_ops(state: &mut DocState, steps: &[Step]) {
    let applied = try_visit_ops(steps, |change, op| {
        let raw = raw_op(change, op);
        state
            .apply_local_op(&raw, op)
            .expect("direct apply was bounds-checked before any write");
        Ok(true)
    });
    debug_assert!(matches!(applied, Ok(true)));
}

fn raw_op<'a>(change: &'a BlockChangeRef, op: &'a Op) -> RawOp<'a> {
    let lamport = change.lamport + (op.counter - change.id.counter) as Lamport;
    let content = match &op.content {
        InnerContent::List(InnerListOp::Insert { slice, pos }) => {
            RawOpContent::List(ListOp::Insert {
                slice: ListSlice::RawData(Cow::Borrowed(slice.as_slice())),
                pos: *pos as usize,
            })
        }
        InnerContent::List(InnerListOp::InsertText {
            slice,
            unicode_len,
            pos,
        }) => {
            let text =
                std::str::from_utf8(slice.as_bytes()).expect("validated UTF-8 before apply_ops");
            RawOpContent::List(ListOp::Insert {
                slice: ListSlice::RawStr {
                    str: Cow::Borrowed(text),
                    unicode_len: *unicode_len as usize,
                },
                pos: *pos as usize,
            })
        }
        InnerContent::List(InnerListOp::Delete(del)) => {
            RawOpContent::List(ListOp::Delete(DeleteSpanWithId::from(*del)))
        }
        InnerContent::Map(set) => RawOpContent::Map(MapSet {
            key: set.key.clone(),
            value: set.value.clone(),
        }),
        #[cfg(feature = "counter")]
        InnerContent::Future(FutureInnerContent::Counter(value)) => RawOpContent::Counter(*value),
        _ => unreachable!("is_direct_op only keeps direct ops"),
    };
    RawOp {
        id: ID::new(change.id.peer, op.counter),
        lamport,
        container: op.container,
        content,
    }
}
