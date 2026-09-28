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
    utils::{
        string_slice::StringSlice,
        utf16::{count_unicode_chars, count_utf16_len},
    },
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
///
/// A counter is aligned by incrementing it here, unless `dry_run` is set. The dry run lets
/// `LoroDoc::_apply_diff` see which edits a full-state batch turns into before applying any.
pub(crate) fn align_full_state(
    handler: &Handler,
    target: Diff,
    current: Diff,
    full_state_targets: &mut FxHashSet<ContainerID>,
    #[cfg_attr(not(feature = "counter"), allow(unused_variables))] dry_run: bool,
) -> LoroResult<Option<Diff>> {
    match (target, current) {
        (Diff::Map(target), Diff::Map(current)) => {
            Ok(align_map(target, current, full_state_targets).map(Diff::Map))
        }
        #[cfg(feature = "counter")]
        (Diff::Counter(target), Diff::Counter(current)) => {
            if let Handler::Counter(counter) = handler {
                let delta = target - current;
                if delta != 0.0 && !dry_run {
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
            let movable = matches!(handler, Handler::MovableList(_));
            Ok(
                match align_list(&target, &current, movable, full_state_targets) {
                    Some(edit) => (!edit.is_empty()).then_some(Diff::List(edit)),
                    None => Some(Diff::List(target)),
                },
            )
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

/// Length of `s` in the units of text diffs (see `StringSlice::rle_len`).
fn text_len(s: &str) -> usize {
    if cfg!(feature = "wasm") {
        count_utf16_len(s.as_bytes())
    } else {
        count_unicode_chars(s.as_bytes())
    }
}

/// Style runs of a pure-insertion text diff, or `None` if it is not one.
fn text_runs(diff: &TextDiff) -> Option<Vec<(&str, &TextMeta)>> {
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
        ans.push((value.as_str(), attr));
    }
    Some(ans)
}

/// The pieces of `runs` that cover the byte range `range` of their concatenated text.
fn slice_runs<'a>(
    runs: &[(&'a str, &'a TextMeta)],
    range: std::ops::Range<usize>,
) -> Vec<(&'a str, &'a TextMeta)> {
    let mut ans = Vec::new();
    let mut offset = 0;
    for &(text, attr) in runs {
        let (start, end) = (offset, offset + text.len());
        offset = end;
        let (from, to) = (range.start.max(start), range.end.min(end));
        if from < to {
            ans.push((&text[from - start..to - start], attr));
        }
    }
    ans
}

/// Lengths of the longest common prefix and of the longest common suffix of the rest.
fn common_ends<T: PartialEq>(a: &[T], b: &[T]) -> (usize, usize) {
    common_ends_by(a, b, |x, y| x == y)
}

fn common_ends_by<T>(a: &[T], b: &[T], eq: impl Fn(&T, &T) -> bool) -> (usize, usize) {
    let prefix = a.iter().zip(b).take_while(|(x, y)| eq(x, y)).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| eq(x, y))
        .count();
    (prefix, suffix)
}

/// The styles to mark on a kept char so that `current` becomes `target` (`null` unmarks).
fn style_change(current: &TextMeta, target: &TextMeta) -> TextMeta {
    let mut change = TextMeta::default();
    for (key, value) in target.0.iter() {
        if current.0.get(key) != Some(value) {
            change.0.insert(key.clone(), value.clone());
        }
    }
    for key in current.0.keys() {
        if !target.0.contains_key(key) {
            change.0.insert(key.clone(), LoroValue::Null);
        }
    }
    change
}

/// Retains a kept text range given as the `current` and `target` runs over the same text,
/// re-marking the pieces whose styles differ.
fn push_restyled_retain(
    edit: &mut TextDiff,
    current: &[(&str, &TextMeta)],
    target: &[(&str, &TextMeta)],
) {
    let (mut ci, mut ti) = (0, 0);
    let (mut c_off, mut t_off) = (0, 0);
    while ci < current.len() && ti < target.len() {
        let (c_text, c_attr) = current[ci];
        let (t_text, t_attr) = target[ti];
        let take = (c_text.len() - c_off).min(t_text.len() - t_off);
        let piece = &c_text[c_off..c_off + take];
        let change = if c_attr == t_attr {
            TextMeta::default()
        } else {
            style_change(c_attr, t_attr)
        };
        edit.push_retain(text_len(piece), change);
        c_off += take;
        t_off += take;
        if c_off == c_text.len() {
            ci += 1;
            c_off = 0;
        }
        if t_off == t_text.len() {
            ti += 1;
            t_off = 0;
        }
    }
}

/// Keeps the text of the common prefix and suffix (compared by content; differing styles are
/// re-marked) and replaces the middle. Works on style runs, so its cost is a byte comparison
/// of the two texts plus the number of runs.
fn align_text(target: &TextDiff, current: &TextDiff) -> Option<TextDiff> {
    let target = text_runs(target)?;
    let current = text_runs(current)?;
    let target_text: String = target.iter().map(|(s, _)| *s).collect();
    let current_text: String = current.iter().map(|(s, _)| *s).collect();
    let (t, c) = (target_text.as_bytes(), current_text.as_bytes());
    let mut prefix = t.iter().zip(c).take_while(|(x, y)| x == y).count();
    while !target_text.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = t[prefix..]
        .iter()
        .rev()
        .zip(c[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    while !target_text.is_char_boundary(t.len() - suffix) {
        suffix -= 1;
    }
    let mut edit = TextDiff::new();
    push_restyled_retain(
        &mut edit,
        &slice_runs(&current, 0..prefix),
        &slice_runs(&target, 0..prefix),
    );
    edit.push_delete(text_len(&current_text[prefix..c.len() - suffix]));
    for (text, attr) in slice_runs(&target, prefix..t.len() - suffix) {
        edit.push_insert(StringSlice::from(text), attr.clone());
    }
    push_restyled_retain(
        &mut edit,
        &slice_runs(&current, c.len() - suffix..c.len()),
        &slice_runs(&target, t.len() - suffix..t.len()),
    );
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
///
/// In a movable list, `apply_delta` turns deleting a child container and re-inserting it into
/// a move (see `movable_list_apply_delta.rs`), so middle children that exist in both lists
/// keep their id as well. A plain list recreates them under fresh ids.
fn align_list(
    target: &ListDiff,
    current: &ListDiff,
    movable: bool,
    full_state_targets: &mut FxHashSet<ContainerID>,
) -> Option<ListDiff> {
    let target = list_items(target)?;
    let current = list_items(current)?;
    let target_values: Vec<LoroValue> = target.iter().map(|v| v.to_value()).collect();
    let current_values: Vec<LoroValue> = current.iter().map(|v| v.to_value()).collect();
    let (prefix, suffix) = common_ends(&target_values, &current_values);
    let mut kept: Vec<&LoroValue> = target_values[..prefix]
        .iter()
        .chain(&target_values[target_values.len() - suffix..])
        .collect();
    if movable {
        let current_middle: FxHashSet<&ContainerID> = current_values
            [prefix..current_values.len() - suffix]
            .iter()
            .filter_map(|v| v.as_container())
            .collect();
        kept.extend(
            target_values[prefix..target_values.len() - suffix]
                .iter()
                .filter(|v| {
                    v.as_container()
                        .is_some_and(|id| current_middle.contains(id))
                }),
        );
    }
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
