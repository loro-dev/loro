//! Text styles of an undo/redo step, corrected from the state's style ops.
//!
//! The transformed inverse of a step is positional: it cannot tell which style
//! keys a collaborator changed after the step (import events report a run's
//! whole style set), nor which concurrently inserted text took the step's style
//! (loro-dev/loro#1229, loro-dev/loro#1009). The state knows both: every
//! character keeps the style ops that cover it. See
//! `context/undo-text-styles.md`.

use loro_common::{ContainerID, IdSpan, LoroValue, ID};
use loro_delta::DeltaItem;
use rustc_hash::FxHashMap;

use crate::{
    container::richtext::{richtext_state::UndoStyleOverrides, StyleOp},
    event::{Diff, TextDiff, TextMeta},
    undo::DiffBatch,
    LoroDoc,
};

/// Correct the retain attributes of the text diffs in `diff`, an undo step about
/// to be applied to the latest state. `spans` are the ops being undone, all
/// written by one peer. For each character and style key:
///
/// - if the winning op is in `spans`, the key gets the value of the latest op
///   covering the character that is not, or `Null` if none is left;
/// - if the winning op is another peer's, the key is left as it is;
/// - otherwise (the step's own peer wrote it later, e.g. text an earlier
///   undo/redo re-inserted) the transformed attribute stands.
///
/// Only text diffs that change a style are looked at.
pub(crate) fn restyle_text_diffs(
    doc: &LoroDoc,
    diff: &mut DiffBatch,
    spans: &[IdSpan],
    container_remap: &FxHashMap<ContainerID, ContainerID>,
) {
    let Some(local_peer) = spans.first().map(|s| s.peer) else {
        return;
    };
    let undone = |op: &StyleOp| spans.iter().any(|s| s.contains(ID::new(op.peer, op.cnt)));
    for (cid, d) in diff.cid_to_events.iter_mut() {
        let Diff::Text(text) = d else {
            continue;
        };
        if !text
            .iter()
            .any(|item| matches!(item, DeltaItem::Retain { attr, .. } if !attr.0.is_empty()))
        {
            continue;
        }
        let mut cid = cid;
        while let Some(next) = container_remap.get(cid) {
            cid = next;
        }
        let Some(idx) = doc.arena.id_to_idx(cid) else {
            continue;
        };
        let runs = doc.state.lock().with_state_mut(idx, |state| {
            state
                .as_richtext_state_mut()
                .and_then(|text| text.undo_style_overrides(&undone, local_peer))
        });
        if let Some(runs) = runs {
            *text = with_style_overrides(text, runs);
        }
    }
}

type Overrides = FxHashMap<String, Option<LoroValue>>;

fn overridden(attr: &TextMeta, overrides: &Overrides) -> TextMeta {
    let mut attr = attr.clone();
    for (key, value) in overrides {
        match value {
            Some(value) => {
                attr.0.insert(key.clone(), value.clone());
            }
            None => {
                attr.0.remove(key);
            }
        }
    }
    attr
}

/// `diff` with `runs` applied to its retains (`Some` sets a key, `None` removes
/// it). `runs` cover the text `diff` applies to; past the end of `diff`, retains
/// are added where a run sets a key.
fn with_style_overrides(diff: &TextDiff, runs: UndoStyleOverrides) -> TextDiff {
    let mut runs = runs.into_iter().filter(|(len, _)| *len > 0).peekable();
    let mut ans = TextDiff::new();
    // How much of the front run is used up.
    let mut used = 0;
    for item in diff.iter() {
        let (mut len, attr) = match item {
            DeltaItem::Retain { len, attr } => (*len, Some(attr)),
            DeltaItem::Replace {
                value,
                attr,
                delete,
            } => {
                ans.push_replace(value.clone(), attr.clone(), *delete);
                (*delete, None)
            }
        };
        while len > 0 {
            let Some((run_len, overrides)) = runs.peek() else {
                // A diff longer than the text: keep it as it is.
                if let Some(attr) = attr {
                    ans.push_retain(len, attr.clone());
                }
                break;
            };
            let n = (run_len - used).min(len);
            if let Some(attr) = attr {
                ans.push_retain(n, overridden(attr, overrides));
            }
            used += n;
            len -= n;
            if used == *run_len {
                runs.next();
                used = 0;
            }
        }
    }
    let empty = TextMeta::default();
    if let Some((run_len, overrides)) = runs.next() {
        ans.push_retain(run_len - used, overridden(&empty, &overrides));
    }
    for (run_len, overrides) in runs {
        ans.push_retain(run_len, overridden(&empty, &overrides));
    }
    ans.chop();
    ans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, LoroValue)]) -> TextMeta {
        TextMeta(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    fn overrides(pairs: &[(&str, Option<LoroValue>)]) -> Overrides {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn overrides_apply_to_retains_and_past_the_end() {
        let mut diff = TextDiff::new();
        diff.push_retain(1, attrs(&[("bold", LoroValue::Null)]));
        diff.push_retain(
            2,
            attrs(&[("bold", LoroValue::Null), ("color", LoroValue::Null)]),
        );
        diff.push_insert("xy".into(), attrs(&[("i", true.into())]));
        diff.push_delete(1);
        let runs = vec![
            (2, overrides(&[("bold", Some(LoroValue::Null))])),
            (2, overrides(&[("color", None)])),
            (3, overrides(&[("color", Some("red".into()))])),
            (2, overrides(&[("color", None)])),
        ];
        let ans = with_style_overrides(&diff, runs);
        let mut expected = TextDiff::new();
        expected.push_retain(1, attrs(&[("bold", LoroValue::Null)]));
        expected.push_retain(
            1,
            attrs(&[("bold", LoroValue::Null), ("color", LoroValue::Null)]),
        );
        expected.push_retain(1, attrs(&[("bold", LoroValue::Null)]));
        expected.push_insert("xy".into(), attrs(&[("i", true.into())]));
        expected.push_delete(1);
        expected.push_retain(3, attrs(&[("color", "red".into())]));
        assert_eq!(ans, expected);
    }
}
