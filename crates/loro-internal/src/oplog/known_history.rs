//! Imported changes that reuse op ids the doc already has.
//!
//! An import skips the part of each change the doc already has (by version vector)
//! and applies the rest on top of the local history. When two clients shared a
//! peer id, the "known" part is different history, and the rest is then applied to
//! a state it was not written against: it panicked inside state apply (poisoning
//! the doc) or silently scrambled it (loro-dev/loro#1118). See
//! `context/import-peer-id-reuse.md`.

use loro_common::{Counter, HasCounterSpan, LoroError, LoroResult, LoroValue, PeerID, ID};
use rle::{HasLength, Sliceable};
use rustc_hash::FxHashMap;

use crate::{
    arena::SharedArena,
    change::Change,
    container::list::list_op::InnerListOp,
    op::{InnerContent, Op},
    version::Frontiers,
    OpLog,
};

#[cfg(feature = "counter")]
use crate::op::FutureInnerContent;

/// How to compare imported op values with known history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportedValues {
    /// Test-only baseline for checking which equivalences Lossy adds. Production
    /// imports all use Lossy because binary history can also have a JSON ancestor.
    #[cfg(test)]
    Exact,
    /// Compare values modulo JSON conversions, recursively, and bypass ambiguous
    /// payload kinds. Binary imports need this too: their history may have been
    /// relayed through JSON earlier.
    Lossy,
}

impl ImportedValues {
    fn eq(self, a: &LoroValue, b: &LoroValue) -> bool {
        #[cfg(test)]
        if self == Self::Exact {
            return a == b;
        }

        match (a, b) {
            (LoroValue::Double(d), LoroValue::I64(i))
            | (LoroValue::I64(i), LoroValue::Double(d)) => *d == *i as f64,
            (LoroValue::String(string), LoroValue::Container(_))
            | (LoroValue::Container(_), LoroValue::String(string)) => {
                // A marker-looking string also becomes a container reference.
                // JSON peer compression can reinterpret its peer as an index,
                // so the decoded id cannot safely be compared with that string.
                loro_common::ContainerID::try_from_loro_value_string(string).is_some()
            }
            (LoroValue::Binary(bytes), LoroValue::List(list))
            | (LoroValue::List(list), LoroValue::Binary(bytes)) => {
                bytes.len() == list.len()
                    && bytes
                        .iter()
                        .zip(list.iter())
                        .all(|(&byte, value)| match value {
                            LoroValue::I64(number) => *number == i64::from(byte),
                            LoroValue::Double(number) => *number == f64::from(byte),
                            _ => false,
                        })
            }
            (LoroValue::Double(number), LoroValue::Null)
            | (LoroValue::Null, LoroValue::Double(number)) => !number.is_finite(),
            (LoroValue::List(a), LoroValue::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| self.eq(a, b))
            }
            (LoroValue::Map(a), LoroValue::Map(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, a)| b.get(key).is_some_and(|b| self.eq(a, b)))
            }
            _ => a == b,
        }
    }
}

impl OpLog {
    /// Check the decoded `changes` against the history this doc already has for the
    /// same op ids, then drop the parts it already has.
    ///
    /// Returns [`LoroError::UsedOpID`] with the first conflicting id when an imported
    /// change differs from the local one (deps or op content), and
    /// [`LoroError::DecodeError`] when a change skips counters of its peer (see
    /// [`Self::check_changes_do_not_skip_counters`]). It reads the op log only, so a
    /// caller can return the error before mutating anything.
    ///
    /// The comparison runs only when the import brings something new: that is what
    /// would be applied on top of a conflicting prefix. An import made only of
    /// known changes is a no-op either way and stays as cheap as before.
    pub(crate) fn check_and_trim_known_part_of_changes(
        &self,
        mut changes: Vec<Change>,
        values: ImportedValues,
    ) -> LoroResult<Vec<Change>> {
        let vv = self.vv();
        let known_end = |c: &Change| vv.get(&c.id.peer).copied().unwrap_or(0);
        if changes.iter().any(|c| c.ctr_end() > known_end(c)) {
            for change in changes.iter() {
                if change.id.counter < known_end(change) {
                    self.check_known_part_of_change(change, known_end(change), values)?;
                }
            }
        }

        changes.retain_mut(|c| {
            let end = known_end(c);
            if c.id.counter >= end {
                true
            } else if c.ctr_end() > end {
                *c = c.slice((end - c.id.counter) as usize, c.atom_len());
                true
            } else {
                false
            }
        });
        self.check_changes_do_not_skip_counters(&changes)?;
        Ok(changes)
    }

    /// A change of peer `p` starting at counter `c > 0` is written after `p@c-1`, so
    /// `p@c-1` is in the causal past of its deps. When all its deps are present but
    /// `p@c-1` is not, the change skips counters and is invalid. The main pass of
    /// `import_changes_to_oplog` applies any change whose deps are present and
    /// would hit the counter assertion in `AppDag::update_version_on_new_change`
    /// under the op log lock. (Changes parked as pending already wait for `p@c-1`,
    /// see `remote_change_apply_state`.)
    ///
    /// This replays that pass on counter ends only: changes are visited in the same
    /// order, and a change counts as applied when its deps are.
    fn check_changes_do_not_skip_counters(&self, changes: &[Change]) -> LoroResult<()> {
        let vv = self.vv();
        let mut ends: FxHashMap<PeerID, Counter> = FxHashMap::default();
        for change in changes {
            let end_of = |ends: &FxHashMap<PeerID, Counter>, peer: &PeerID| {
                ends.get(peer)
                    .or_else(|| vv.get(peer))
                    .copied()
                    .unwrap_or(0)
            };
            let peer = change.id.peer;
            let end = end_of(&ends, &peer);
            if change.ctr_end() <= end || self.dag.import_deps_before_shallow_root(&change.deps) {
                continue;
            }

            if !change
                .deps
                .iter()
                .all(|dep| dep.counter < end_of(&ends, &dep.peer))
            {
                // Parked as pending.
                continue;
            }

            if change.id.counter > end {
                return Err(LoroError::DecodeError(
                    format!(
                        "Invalid change {}: its deps are present but {} is not, so it skips counters of its peer",
                        change.id,
                        ID::new(peer, change.id.counter - 1),
                    )
                    .into_boxed_str(),
                ));
            }

            ends.insert(peer, change.ctr_end());
        }

        Ok(())
    }

    fn check_known_part_of_change(
        &self,
        change: &Change,
        known_end: Counter,
        values: ImportedValues,
    ) -> LoroResult<()> {
        let peer = change.id.peer;
        // History before the shallow root is not stored, so it cannot be compared.
        let shallow_start = self.shallow_since_vv().get(&peer).copied().unwrap_or(0);
        let end = change.ctr_end().min(known_end);
        let mut ctr = change.id.counter.max(shallow_start);
        while ctr < end {
            // Missing local history cannot be compared; keep the old behavior for it.
            let Some(local) = self.change_store.get_change(ID::new(peer, ctr)) else {
                break;
            };
            let seg_end = end.min(local.ctr_end());
            if seg_end <= ctr {
                break;
            }

            if deps_at(change, ctr) != deps_at(&local, ctr) {
                return Err(LoroError::UsedOpID {
                    id: ID::new(peer, ctr),
                });
            }

            if let Some(bad) = first_mismatch(&self.arena, values, change, &local, ctr, seg_end) {
                return Err(LoroError::UsedOpID {
                    id: ID::new(peer, bad),
                });
            }

            ctr = seg_end;
        }

        Ok(())
    }
}

/// The deps of the op at `ctr`. Inside a change every op depends on the previous
/// one, and changes only merge when that holds, so this does not depend on how
/// either side split its history into changes.
fn deps_at(change: &Change, ctr: Counter) -> Frontiers {
    if ctr == change.id.counter {
        change.deps.clone()
    } else {
        Frontiers::from_id(ID::new(change.id.peer, ctr - 1))
    }
}

/// Walk the ops of `a` and `b` over `[start, end)` and return the first counter at
/// which they differ. Each side may split or merge the ops differently, so both
/// are cut at every boundary of either side before comparing.
fn first_mismatch(
    arena: &SharedArena,
    values: ImportedValues,
    a: &Change,
    b: &Change,
    start: Counter,
    end: Counter,
) -> Option<Counter> {
    let mut a_ops = OpCursor::new(a, start);
    let mut b_ops = OpCursor::new(b, start);
    let mut ctr = start;
    while ctr < end {
        let (Some(a_op), Some(b_op)) = (a_ops.op_at(ctr), b_ops.op_at(ctr)) else {
            return Some(ctr);
        };
        let len = (end - ctr)
            .min(a_op.ctr_end() - ctr)
            .min(b_op.ctr_end() - ctr);
        let a_slice = slice_op(a_op, ctr, len);
        let b_slice = slice_op(b_op, ctr, len);
        if !op_eq(arena, values, &a_slice, &b_slice) {
            // Error path only: find the exact atom.
            let offset = (0..len)
                .find(|&i| {
                    !op_eq(
                        arena,
                        values,
                        &slice_op(a_op, ctr + i, 1),
                        &slice_op(b_op, ctr + i, 1),
                    )
                })
                .unwrap_or(0);
            return Some(ctr + offset);
        }
        ctr += len;
    }

    None
}

struct OpCursor<'a> {
    ops: &'a [Op],
    index: usize,
}

impl<'a> OpCursor<'a> {
    fn new(change: &'a Change, start: Counter) -> Self {
        let ops = change.ops.vec().as_slice();
        let index = ops.partition_point(|op| op.ctr_end() <= start);
        Self { ops, index }
    }

    /// The op containing `ctr`. Counters only move forward.
    fn op_at(&mut self, ctr: Counter) -> Option<&'a Op> {
        while let Some(op) = self.ops.get(self.index) {
            if op.ctr_end() > ctr {
                return (op.counter <= ctr).then_some(op);
            }
            self.index += 1;
        }
        None
    }
}

fn slice_op(op: &Op, ctr: Counter, len: Counter) -> std::borrow::Cow<'_, Op> {
    let from = (ctr - op.counter) as usize;
    let to = from + len as usize;
    if from == 0 && to == op.atom_len() {
        std::borrow::Cow::Borrowed(op)
    } else {
        std::borrow::Cow::Owned(op.slice(from, to))
    }
}

/// Whether two ops with the same id range are the same op. Compares what the op
/// means, not how it is stored: arena offsets and the direction of a one-atom
/// delete are representation details.
fn op_eq(arena: &SharedArena, values: ImportedValues, a: &Op, b: &Op) -> bool {
    if a.container != b.container || a.atom_len() != b.atom_len() {
        return false;
    }

    match (&a.content, &b.content) {
        (InnerContent::List(a), InnerContent::List(b)) => list_op_eq(arena, values, a, b),
        (InnerContent::Map(a), InnerContent::Map(b)) => {
            a.key == b.key
                && match (&a.value, &b.value) {
                    (Some(a), Some(b)) => values.eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
        }
        (InnerContent::Tree(a), InnerContent::Tree(b)) => a == b,
        (InnerContent::Future(a), InnerContent::Future(b)) => match (a, b) {
            #[cfg(feature = "counter")]
            (FutureInnerContent::Counter(a), FutureInnerContent::Counter(b)) => {
                a == b || (a.is_nan() && b.is_nan())
            }
            (
                crate::op::FutureInnerContent::Unknown {
                    prop: a_prop,
                    value: _a_value,
                },
                crate::op::FutureInnerContent::Unknown {
                    prop: b_prop,
                    value: _b_value,
                },
            ) => {
                // A newer container's payload may contain LoroValues or arena
                // indices whose meaning this version cannot interpret. Keep the
                // old JSON value bypass for this kind, including binary relays.
                a_prop == b_prop
                    && match values {
                        ImportedValues::Lossy => true,
                        #[cfg(test)]
                        ImportedValues::Exact => _a_value == _b_value,
                    }
            }
            #[allow(unreachable_patterns)]
            _ => false,
        },
        _ => false,
    }
}

fn list_op_eq(
    arena: &SharedArena,
    values: ImportedValues,
    a: &InnerListOp,
    b: &InnerListOp,
) -> bool {
    match (a, b) {
        (
            InnerListOp::Insert {
                slice: a_slice,
                pos: a_pos,
            },
            InnerListOp::Insert {
                slice: b_slice,
                pos: b_pos,
            },
        ) => {
            a_pos == b_pos
                && arena.value_slices_eq(a_slice.to_range(), b_slice.to_range(), |a, b| {
                    values.eq(a, b)
                })
        }
        (
            InnerListOp::InsertText {
                slice: a_slice,
                unicode_len: a_len,
                pos: a_pos,
                ..
            },
            InnerListOp::InsertText {
                slice: b_slice,
                unicode_len: b_len,
                pos: b_pos,
                ..
            },
        ) => a_pos == b_pos && a_len == b_len && a_slice[..] == b_slice[..],
        (InnerListOp::Delete(a), InnerListOp::Delete(b)) => {
            // A one-atom delete has no direction: `signed_len` 1 and -1 delete the
            // same element, and which one is stored depends on how it was sliced.
            a.id_start == b.id_start
                && a.span.pos == b.span.pos
                && (a.span.signed_len == b.span.signed_len
                    || (a.span.signed_len.abs() == 1 && b.span.signed_len.abs() == 1))
        }
        (
            InnerListOp::Move {
                from: a_from,
                elem_id: a_elem,
                to: a_to,
            },
            InnerListOp::Move {
                from: b_from,
                elem_id: b_elem,
                to: b_to,
            },
        ) => a_from == b_from && a_elem == b_elem && a_to == b_to,
        (
            InnerListOp::Set {
                elem_id: a_elem,
                value: a_value,
            },
            InnerListOp::Set {
                elem_id: b_elem,
                value: b_value,
            },
        ) => a_elem == b_elem && values.eq(a_value, b_value),
        (
            InnerListOp::StyleStart {
                start: a_start,
                end: a_end,
                key: a_key,
                value: a_value,
                info: a_info,
            },
            InnerListOp::StyleStart {
                start: b_start,
                end: b_end,
                key: b_key,
                value: b_value,
                info: b_info,
            },
        ) => {
            a_start == b_start
                && a_end == b_end
                && a_key == b_key
                && values.eq(a_value, b_value)
                && a_info == b_info
        }
        (InnerListOp::StyleEnd, InnerListOp::StyleEnd) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ImportedValues;
    use loro_common::LoroValue;

    fn assert_lossy_pair(a: &LoroValue, b: &LoroValue, equal: bool) {
        assert_eq!(ImportedValues::Lossy.eq(a, b), equal, "{a:?} vs {b:?}");
        assert_eq!(ImportedValues::Lossy.eq(b, a), equal, "{b:?} vs {a:?}");
    }

    #[test]
    fn byte_lists_must_match_every_byte() {
        let binary = LoroValue::from(vec![0u8, 1, 255]);
        for list in [
            LoroValue::from(vec![0, 1, 255]),
            LoroValue::from(vec![0.0, 1.0, 255.0]),
        ] {
            assert_lossy_pair(&binary, &list, true);
            assert!(!ImportedValues::Exact.eq(&binary, &list));
        }
        assert_lossy_pair(
            &LoroValue::from(Vec::<u8>::new()),
            &LoroValue::from(Vec::<i32>::new()),
            true,
        );
        for list in [
            LoroValue::from(vec![0, 1]),
            LoroValue::from(vec![0, 1, 255, 0]),
            LoroValue::from(vec![0, 1, 254]),
            LoroValue::from(vec![0, -1, 255]),
            LoroValue::from(vec![0, 1, 256]),
            LoroValue::from(vec![0.0, 1.5, 255.0]),
            LoroValue::from(vec![0.0, f64::NAN, 255.0]),
            LoroValue::from(vec![
                LoroValue::I64(0),
                LoroValue::Bool(true),
                LoroValue::I64(255),
            ]),
        ] {
            assert_lossy_pair(&binary, &list, false);
        }
        assert_lossy_pair(&binary, &LoroValue::from(vec![0u8, 1, 254]), false);
    }

    #[test]
    fn only_non_finite_numbers_match_null() {
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let number = LoroValue::Double(number);
            assert_lossy_pair(&number, &LoroValue::Null, true);
            assert!(!ImportedValues::Exact.eq(&number, &LoroValue::Null));
            assert!(ImportedValues::Exact.eq(&number, &number));
        }
        for number in [0.0, -0.0, 1.5, f64::MAX] {
            assert_lossy_pair(&LoroValue::Double(number), &LoroValue::Null, false);
        }
        assert_lossy_pair(&LoroValue::I64(0), &LoroValue::Null, false);
        assert_lossy_pair(
            &LoroValue::Double(f64::INFINITY),
            &LoroValue::Double(f64::NEG_INFINITY),
            false,
        );
    }

    #[test]
    fn nested_values_allow_only_json_representations() {
        let nested =
            |value| LoroValue::Map(vec![("key".into(), LoroValue::from(vec![value]))].into());
        let original = nested(LoroValue::from(vec![
            LoroValue::from(vec![1u8, 2]),
            LoroValue::Double(f64::NAN),
        ]));
        let json: LoroValue =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_lossy_pair(&original, &json, true);
        assert!(!ImportedValues::Exact.eq(&original, &json));
        assert_lossy_pair(
            &original,
            &nested(LoroValue::from(vec![
                LoroValue::from(vec![1, 3]),
                LoroValue::Null,
            ])),
            false,
        );
        assert_lossy_pair(
            &original,
            &LoroValue::Map(vec![("other".into(), original["key"].clone())].into()),
            false,
        );
        assert_lossy_pair(&original, &LoroValue::Map(Default::default()), false);
        assert_lossy_pair(
            &LoroValue::from(vec![1, 2]),
            &LoroValue::from(vec![1, 2, 3]),
            false,
        );
    }

    #[test]
    fn ordinary_values_still_require_exact_equality() {
        let values = [
            LoroValue::Null,
            LoroValue::Bool(false),
            LoroValue::Bool(true),
            LoroValue::I64(1),
            LoroValue::I64(2),
            LoroValue::Double(1.5),
            LoroValue::from("one"),
            LoroValue::from("two"),
            LoroValue::from(loro_common::ContainerID::new_root(
                "a",
                loro_common::ContainerType::Map,
            )),
            LoroValue::from(loro_common::ContainerID::new_root(
                "b",
                loro_common::ContainerType::Map,
            )),
        ];
        for a in &values {
            for b in &values {
                assert_lossy_pair(a, b, a == b);
            }
        }
    }

    #[test]
    fn integral_doubles_match_only_the_same_integer() {
        for integer in [0, 2, -2, 9_007_199_254_740_991, i64::MIN, i64::MAX] {
            let double = LoroValue::Double(integer as f64);
            let integer = LoroValue::I64(integer);
            assert_lossy_pair(&double, &integer, true);
            assert!(!ImportedValues::Exact.eq(&double, &integer));
        }
        assert_lossy_pair(&LoroValue::Double(-0.0), &LoroValue::I64(0), true);
        for number in [2.5, 3.0, f64::NAN, f64::INFINITY] {
            assert_lossy_pair(&LoroValue::Double(number), &LoroValue::I64(2), false);
        }
    }

    #[test]
    fn container_marker_strings_are_ambiguous_after_peer_compression() {
        use loro_common::{ContainerID, ContainerType};
        let id = ContainerID::new_root("target", ContainerType::Text);
        let marker = LoroValue::from(id.to_loro_value_string());
        let container = LoroValue::Container(id.clone());
        assert_lossy_pair(&marker, &container, true);
        assert!(!ImportedValues::Exact.eq(&marker, &container));
        for string in ["plain text", "🦜:invalid"] {
            assert_lossy_pair(&LoroValue::from(string), &container, false);
        }
        assert_lossy_pair(
            &LoroValue::from("🦜:cid:0@0:Text"),
            &LoroValue::Container(ContainerID::new_normal(
                loro_common::ID::new(1, 0),
                ContainerType::Text,
            )),
            true,
        );
        assert_lossy_pair(&marker, &LoroValue::from("🦜:cid:root-other:Text"), false);
        assert_lossy_pair(
            &container,
            &LoroValue::Container(ContainerID::new_root("other", ContainerType::Text)),
            false,
        );
    }

    #[test]
    fn unknown_payload_bypass_still_compares_props_and_containers() {
        use crate::{
            arena::SharedArena,
            encoding::OwnedValue,
            op::{FutureInnerContent, InnerContent, Op},
        };
        use loro_common::{ContainerID, ContainerType};
        let arena = SharedArena::new();
        let container =
            arena.register_container(&ContainerID::new_root("future", ContainerType::Unknown(9)));
        let make_op = |prop, value| Op {
            counter: 0,
            container,
            content: InnerContent::Future(FutureInnerContent::Unknown {
                prop,
                value: Box::new(value),
            }),
        };
        let a = make_op(1, OwnedValue::LoroValue(LoroValue::Double(2.0)));
        let b = make_op(1, OwnedValue::LoroValue(LoroValue::I64(2)));
        assert!(super::op_eq(&arena, ImportedValues::Lossy, &a, &b));
        assert!(!super::op_eq(&arena, ImportedValues::Exact, &a, &b));
        let different_prop = make_op(2, OwnedValue::LoroValue(LoroValue::I64(2)));
        assert!(!super::op_eq(
            &arena,
            ImportedValues::Lossy,
            &a,
            &different_prop
        ));
        let mut different_container = b;
        different_container.container =
            arena.register_container(&ContainerID::new_root("other", ContainerType::Unknown(9)));
        assert!(!super::op_eq(
            &arena,
            ImportedValues::Lossy,
            &a,
            &different_container
        ));
    }

    #[cfg(feature = "counter")]
    #[test]
    fn counter_values_keep_numeric_equality_in_lossy_mode() {
        use crate::{
            arena::SharedArena,
            op::{FutureInnerContent, InnerContent, Op},
        };
        use loro_common::{ContainerID, ContainerType};
        let arena = SharedArena::new();
        let container =
            arena.register_container(&ContainerID::new_root("counter", ContainerType::Counter));
        let make_op = |value| Op {
            counter: 0,
            container,
            content: InnerContent::Future(FutureInnerContent::Counter(value)),
        };
        for (a, b, equal) in [
            (2.0, 2.0, true),
            (2.0, 3.0, false),
            (-0.0, 0.0, true),
            (f64::NAN, f64::NAN, true),
            (f64::INFINITY, f64::INFINITY, true),
            (f64::NAN, f64::INFINITY, false),
        ] {
            for values in [ImportedValues::Lossy, ImportedValues::Exact] {
                assert_eq!(
                    super::op_eq(&arena, values, &make_op(a), &make_op(b)),
                    equal
                );
            }
        }
    }
}
