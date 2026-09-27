//! Applying a full-state diff to a re-activated mergeable child in `LoroDoc::apply_diff`.
//!
//! Events and `LoroDoc::diff` describe a container that becomes visible with its full state.
//! `apply_diff` recreates a regular container under a fresh id, so a full state is exactly
//! right for it. A mergeable child is re-activated at its deterministic cid, which may already
//! hold hidden state in the target doc (the same, different, or none). [`align_full_state`]
//! turns the full state into an edit of that hidden state, keeping the characters, elements,
//! entries and tree nodes the two share. See context/mergeable-containers.md.

use super::{Handler, ValueOrHandler};
use crate::{
    delta::{ResolvedMapDelta, ResolvedMapValue, TreeDiff, TreeDiffItem, TreeExternalDiff},
    event::{Diff, ListDeltaMeta, ListDiff, TextDiff, TextMeta},
    state::TreeParentId,
    utils::string_slice::StringSlice,
};
use fractional_index::FractionalIndex;
use loro_common::{ContainerID, InternalString, LoroResult, LoroValue, TreeID};
use loro_delta::{array_vec::ArrayVec, DeltaItem, DeltaRope};
use rustc_hash::{FxHashMap, FxHashSet};

/// Given `target`, a full-state diff for `handler`, and `current`, the handler's current state
/// as a full-state diff, returns the edit that turns `current` into `target` (or applies it
/// directly). Returns `target` unchanged if it is not a full-state diff.
///
/// Child containers that keep their id are added to `full_state_targets`: their own diffs in
/// the batch are full states as well.
pub(crate) fn align_full_state(
    handler: &Handler,
    target: Diff,
    current: Diff,
    full_state_targets: &mut FxHashSet<ContainerID>,
) -> LoroResult<Option<Diff>> {
    #[cfg(not(feature = "counter"))]
    let _ = handler;
    match (target, current) {
        (Diff::Map(target), Diff::Map(current)) => {
            Ok(align_map(target, current, full_state_targets).map(Diff::Map))
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
        (Diff::Text(target), Diff::Text(current)) => Ok(match align_text(&target, &current) {
            Some(edit) => (!edit.is_empty()).then_some(Diff::Text(edit)),
            None => Some(Diff::Text(target)),
        }),
        (Diff::List(target), Diff::List(current)) => {
            Ok(match align_list(&target, &current, full_state_targets) {
                Some(edit) => (!edit.is_empty()).then_some(Diff::List(edit)),
                None => Some(Diff::List(target)),
            })
        }
        (Diff::Tree(target), Diff::Tree(current)) => {
            Ok(match align_tree(&target, &current, full_state_targets) {
                Some(edit) => (!edit.diff.is_empty()).then_some(Diff::Tree(edit)),
                None => Some(Diff::Tree(target)),
            })
        }
        (Diff::Unknown, _) => Ok(None),
        (target, _) => Ok(Some(target)),
    }
}

fn map_value(v: &ResolvedMapValue) -> Option<LoroValue> {
    v.value.as_ref().map(ValueOrHandler::to_value)
}

/// Per key, so entries (and the child containers they hold) that already match are kept.
fn align_map(
    target: ResolvedMapDelta,
    current: ResolvedMapDelta,
    full_state_targets: &mut FxHashSet<ContainerID>,
) -> Option<ResolvedMapDelta> {
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
    (!updated.is_empty()).then_some(ResolvedMapDelta { updated })
}

/// Length of one char in the units of text diffs (see `StringSlice::rle_len`).
fn char_len(c: char) -> usize {
    if cfg!(feature = "wasm") {
        c.len_utf16()
    } else {
        1
    }
}

/// `None` if `diff` is not a pure insertion.
fn text_chars(diff: &TextDiff) -> Option<Vec<(char, &TextMeta)>> {
    let mut ans = Vec::new();
    for item in diff.iter() {
        let DeltaItem::Replace {
            value,
            attr,
            delete: 0,
        } = item
        else {
            return None;
        };
        ans.extend(value.as_str().chars().map(|c| (c, attr)));
    }
    Some(ans)
}

/// Lengths of the longest common prefix and of the longest common suffix of the rest.
fn common_ends<T: PartialEq>(a: &[T], b: &[T]) -> (usize, usize) {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    (prefix, suffix)
}

/// Keeps the common prefix and suffix (chars with equal styles) and replaces the middle.
fn align_text(target: &TextDiff, current: &TextDiff) -> Option<TextDiff> {
    let target = text_chars(target)?;
    let current = text_chars(current)?;
    let (prefix, suffix) = common_ends(&target, &current);
    let mut edit = TextDiff::new();
    if prefix == target.len() && prefix == current.len() {
        return Some(edit);
    }
    let retain: usize = current[..prefix].iter().map(|(c, _)| char_len(*c)).sum();
    let delete: usize = current[prefix..current.len() - suffix]
        .iter()
        .map(|(c, _)| char_len(*c))
        .sum();
    edit.push_retain(retain, TextMeta::default());
    edit.push_delete(delete);
    let middle = &target[prefix..target.len() - suffix];
    let mut i = 0;
    while i < middle.len() {
        let attr = middle[i].1;
        let run: String = middle[i..]
            .iter()
            .take_while(|(_, a)| *a == attr)
            .map(|(c, _)| *c)
            .collect();
        i += run.chars().count();
        edit.push_insert(StringSlice::from(run), attr.clone());
    }
    Some(edit)
}

/// `None` if `diff` is not a pure insertion.
fn list_items(diff: &ListDiff) -> Option<Vec<&ValueOrHandler>> {
    let mut ans = Vec::new();
    for item in diff.iter() {
        let DeltaItem::Replace {
            value, delete: 0, ..
        } = item
        else {
            return None;
        };
        ans.extend(value.iter());
    }
    Some(ans)
}

/// Keeps the common prefix and suffix (equal values, containers by id) and replaces the middle.
fn align_list(
    target: &ListDiff,
    current: &ListDiff,
    full_state_targets: &mut FxHashSet<ContainerID>,
) -> Option<ListDiff> {
    let target = list_items(target)?;
    let current = list_items(current)?;
    let target_values: Vec<LoroValue> = target.iter().map(|v| v.to_value()).collect();
    let current_values: Vec<LoroValue> = current.iter().map(|v| v.to_value()).collect();
    let (prefix, suffix) = common_ends(&target_values, &current_values);
    let kept = target_values[..prefix]
        .iter()
        .chain(&target_values[target_values.len() - suffix..]);
    for v in kept {
        if let LoroValue::Container(id) = v {
            full_state_targets.insert(id.clone());
        }
    }
    let mut edit: ListDiff = DeltaRope::new();
    if prefix == target.len() && prefix == current.len() {
        return Some(edit);
    }
    edit.push_retain(prefix, ListDeltaMeta::default());
    edit.push_delete(current.len() - prefix - suffix);
    for v in &target[prefix..target.len() - suffix] {
        edit.push_insert(ArrayVec::from([(*v).clone()]), ListDeltaMeta::default());
    }
    Some(edit)
}

/// Keeps nodes by `TreeID`: creates missing ones, moves kept ones whose parent or position
/// differ, and deletes the rest.
fn align_tree(
    target: &TreeDiff,
    current: &TreeDiff,
    full_state_targets: &mut FxHashSet<ContainerID>,
) -> Option<TreeDiff> {
    let mut current_nodes: FxHashMap<TreeID, (TreeParentId, &FractionalIndex, usize)> =
        FxHashMap::default();
    for item in current.diff.iter() {
        let TreeExternalDiff::Create {
            parent,
            index,
            position,
        } = &item.action
        else {
            return None;
        };
        current_nodes.insert(item.target, (*parent, position, *index));
    }
    let mut edit = TreeDiff::default();
    let mut target_nodes = FxHashSet::default();
    for item in target.diff.iter() {
        let TreeExternalDiff::Create {
            parent, position, ..
        } = &item.action
        else {
            return None;
        };
        target_nodes.insert(item.target);
        match current_nodes.get(&item.target) {
            Some((p, pos, _)) if p == parent && *pos == position => {
                full_state_targets.insert(item.target.associated_meta_container());
            }
            Some(_) => {
                // `apply_diff` moves a `Create` target that is still alive.
                full_state_targets.insert(item.target.associated_meta_container());
                edit.diff.push(item.clone());
            }
            None => edit.diff.push(item.clone()),
        }
    }
    for (id, (parent, _, index)) in current_nodes {
        if !target_nodes.contains(&id) {
            edit.diff.push(TreeDiffItem {
                target: id,
                action: TreeExternalDiff::Delete {
                    old_parent: parent,
                    old_index: index,
                },
            });
        }
    }
    Some(edit)
}
