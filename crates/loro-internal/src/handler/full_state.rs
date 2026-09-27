//! Applying a "revival" diff to a container that may already hold state.
//!
//! When a container becomes reachable again between two versions, the recorded diff
//! (events, [`crate::undo::DiffBatch`]) carries the container's *full* target state as a
//! from-empty diff. `apply_diff` recreates regular containers under fresh ids, so that is
//! enough for them. A mergeable child, however, is re-activated at its deterministic cid,
//! which still holds the hidden state; applying the full diff on top of it would add the
//! content twice. [`reconcile_full_state`] turns such a diff into the edit that moves the
//! current state to the target. See `context/mergeable-containers.md`.

use super::{Handler, ValueOrHandler};
use crate::{
    delta::{ResolvedMapDelta, ResolvedMapValue, TreeExternalDiff},
    event::{Diff, TextMeta},
    state::TreeParentId,
};
use fractional_index::FractionalIndex;
use loro_common::{ContainerID, InternalString, LoroResult, LoroValue, TreeID};
use loro_delta::DeltaItem;
use rustc_hash::{FxHashMap, FxHashSet};

/// Canonical form of a from-empty container diff, independent of how the diff is chunked.
#[derive(PartialEq)]
enum FullState {
    Text(Vec<(String, TextMeta)>),
    List(Vec<LoroValue>),
    Tree(FxHashMap<TreeID, (TreeParentId, FractionalIndex)>),
    Unknown,
}

impl FullState {
    /// Returns `None` if `diff` is not a pure insertion diff.
    fn from_diff(diff: &Diff) -> Option<Self> {
        match diff {
            Diff::Text(delta) => {
                let mut ans: Vec<(String, TextMeta)> = Vec::new();
                for item in delta.iter() {
                    let DeltaItem::Replace {
                        value,
                        attr,
                        delete: 0,
                    } = item
                    else {
                        return None;
                    };
                    match ans.last_mut() {
                        Some((s, last_attr)) if last_attr == attr => s.push_str(value.as_str()),
                        _ => ans.push((value.as_str().to_string(), attr.clone())),
                    }
                }
                Some(Self::Text(ans))
            }
            Diff::List(delta) => {
                let mut ans = Vec::new();
                for item in delta.iter() {
                    let DeltaItem::Replace {
                        value, delete: 0, ..
                    } = item
                    else {
                        return None;
                    };
                    ans.extend(value.iter().map(ValueOrHandler::to_value));
                }
                Some(Self::List(ans))
            }
            Diff::Tree(tree) => {
                let mut ans = FxHashMap::default();
                for item in tree.iter() {
                    let TreeExternalDiff::Create {
                        parent, position, ..
                    } = &item.action
                    else {
                        return None;
                    };
                    ans.insert(item.target, (*parent, position.clone()));
                }
                Some(Self::Tree(ans))
            }
            Diff::Unknown => Some(Self::Unknown),
            Diff::Map(_) => unreachable!("maps are reconciled per key"),
            #[cfg(feature = "counter")]
            Diff::Counter(_) => unreachable!("counters are reconciled by value"),
        }
    }
}

/// Containers referenced by a full-state diff. Their own diffs in the same batch are full
/// states as well.
fn collect_children(diff: &Diff, out: &mut FxHashSet<ContainerID>) {
    match diff {
        Diff::List(delta) => {
            for item in delta.iter() {
                if let DeltaItem::Replace { value, .. } = item {
                    for v in value.iter() {
                        if let ValueOrHandler::Handler(h) = v {
                            out.insert(h.id());
                        }
                    }
                }
            }
        }
        Diff::Tree(tree) => {
            for item in tree.iter() {
                out.insert(item.target.associated_meta_container());
            }
        }
        _ => {}
    }
}

fn map_value(v: &ResolvedMapValue) -> Option<LoroValue> {
    v.value.as_ref().map(ValueOrHandler::to_value)
}

/// Given `target`, a from-empty diff describing the state `handler` should end up in, and
/// `current`, the handler's current state as a from-empty diff, returns the diff that
/// still has to be applied (or applies it directly when that is simpler).
///
/// Child containers that keep their id and whose diffs in the batch are therefore also
/// full states are added to `full_state_targets`.
pub(crate) fn reconcile_full_state(
    handler: &Handler,
    target: Diff,
    current: Diff,
    full_state_targets: &mut FxHashSet<ContainerID>,
) -> LoroResult<Option<Diff>> {
    match (target, current) {
        (Diff::Map(target), Diff::Map(current)) => {
            // Per key, so unchanged entries (and the child containers they hold) survive.
            let current: FxHashMap<InternalString, LoroValue> = current
                .updated
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), map_value(v)?)))
                .collect();
            let mut updated = FxHashMap::default();
            let mut target_keys = FxHashSet::default();
            for (key, value) in target.updated {
                let Some(target_value) = map_value(&value) else {
                    continue;
                };
                target_keys.insert(key.clone());
                if current.get(&key) == Some(&target_value) {
                    if let LoroValue::Container(id) = target_value {
                        full_state_targets.insert(id);
                    }
                } else {
                    updated.insert(key, value);
                }
            }
            for key in current.keys() {
                if !target_keys.contains(key) {
                    updated.insert(key.clone(), ResolvedMapValue::new_unset());
                }
            }
            Ok((!updated.is_empty()).then_some(Diff::Map(ResolvedMapDelta { updated })))
        }
        #[cfg(feature = "counter")]
        (Diff::Counter(target), Diff::Counter(current)) => {
            if let Handler::Counter(counter) = handler {
                let delta = target - current;
                if delta != 0.0 {
                    counter.increment(delta)?;
                }
            }
            Ok(None)
        }
        (target, current) => {
            let (Some(target_state), Some(current_state)) = (
                FullState::from_diff(&target),
                FullState::from_diff(&current),
            ) else {
                // Not a revival diff; apply it as is.
                return Ok(Some(target));
            };
            if target_state == current_state {
                collect_children(&target, full_state_targets);
                return Ok(None);
            }
            if !is_empty(&current_state) {
                handler.clear()?;
            }
            Ok(Some(target))
        }
    }
}

fn is_empty(state: &FullState) -> bool {
    match state {
        FullState::Text(t) => t.is_empty(),
        FullState::List(l) => l.is_empty(),
        FullState::Tree(t) => t.is_empty(),
        FullState::Unknown => true,
    }
}
