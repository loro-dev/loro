//! `apply_diff` on a movable list turns "delete a child, insert the same child"
//! into a move, and undo/redo replays diffs through the same code path.

use loro::{
    event::{Diff, DiffBatch, ListDiffItem},
    ContainerID, ContainerTrait, ContainerType, ExportMode, LoroDoc, LoroMap, LoroMovableList,
    LoroResult, LoroText, LoroValue, TextDelta, ToJson, UndoManager, ValueOrContainer, ID,
};
use pretty_assertions::assert_eq;
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use serde_json::{json, Value};

fn cid(id: ContainerID) -> ValueOrContainer {
    ValueOrContainer::Value(LoroValue::Container(id))
}

fn val(v: impl Into<LoroValue>) -> ValueOrContainer {
    ValueOrContainer::Value(v.into())
}

fn ins(values: Vec<ValueOrContainer>) -> ListDiffItem {
    ListDiffItem::Insert {
        insert: values,
        is_move: false,
    }
}

fn del(n: usize) -> ListDiffItem {
    ListDiffItem::Delete { delete: n }
}

fn retain(n: usize) -> ListDiffItem {
    ListDiffItem::Retain { retain: n }
}

fn apply_list_diff(doc: &LoroDoc, target: ContainerID, items: Vec<ListDiffItem>) -> LoroResult<()> {
    let mut batch = DiffBatch::default();
    batch.push(target, Diff::List(items)).unwrap();
    doc.apply_diff(batch)?;
    doc.commit();
    Ok(())
}

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn text_list(doc: &LoroDoc, contents: &[&str]) -> LoroResult<(LoroMovableList, Vec<LoroText>)> {
    let list = doc.get_movable_list("l");
    let mut texts = Vec::new();
    for s in contents {
        let t = list.push_container(LoroText::new())?;
        t.insert(0, s)?;
        texts.push(t);
    }
    doc.commit();
    Ok((list, texts))
}

/// Applies `items`, checks the result, then checks that undo restores the
/// previous value and redo restores `expected` again.
fn apply_then_undo_redo(
    doc: &LoroDoc,
    target: ContainerID,
    items: Vec<ListDiffItem>,
    expected: Value,
) -> LoroResult<()> {
    let before = deep_json(doc);
    let mut undo = UndoManager::new(doc);
    undo.set_merge_interval(0);
    apply_list_diff(doc, target, items)?;
    assert_eq!(deep_json(doc), expected);
    assert!(undo.undo()?);
    assert_eq!(deep_json(doc), before);
    assert!(undo.redo()?);
    assert_eq!(deep_json(doc), expected);
    Ok(())
}

#[test]
fn apply_diff_swaps_two_child_containers() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let (list, t) = text_list(&doc, &["A", "B"])?;
    apply_then_undo_redo(
        &doc,
        list.id(),
        vec![del(2), ins(vec![cid(t[1].id()), cid(t[0].id())])],
        json!({"l": ["B", "A"]}),
    )?;
    // The children are moved, not recreated.
    assert_eq!(list.get(0).unwrap().as_container().unwrap().id(), t[1].id());
    assert_eq!(list.get(1).unwrap().as_container().unwrap().id(), t[0].id());
    Ok(())
}

#[test]
fn apply_diff_rotates_child_containers() -> LoroResult<()> {
    for order in [[1, 2, 0], [2, 0, 1], [2, 1, 0], [0, 2, 1]] {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let (list, t) = text_list(&doc, &["A", "B", "C"])?;
        let names = ["A", "B", "C"];
        apply_then_undo_redo(
            &doc,
            list.id(),
            vec![del(3), ins(order.iter().map(|&i| cid(t[i].id())).collect())],
            json!({"l": order.iter().map(|&i| names[i]).collect::<Vec<_>>()}),
        )?;
        for (pos, &i) in order.iter().enumerate() {
            assert_eq!(
                list.get(pos).unwrap().as_container().unwrap().id(),
                t[i].id()
            );
        }
    }
    Ok(())
}

#[test]
fn apply_diff_moves_children_mixed_with_scalars() -> LoroResult<()> {
    let setup = || -> LoroResult<(LoroDoc, LoroMovableList, LoroText, LoroText, LoroText)> {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let list = doc.get_movable_list("l");
        let a = list.push_container(LoroText::new())?;
        a.insert(0, "A")?;
        list.push("x")?;
        let b = list.push_container(LoroText::new())?;
        b.insert(0, "B")?;
        list.push("y")?;
        let c = list.push_container(LoroText::new())?;
        c.insert(0, "C")?;
        doc.commit();
        Ok((doc, list, a, b, c))
    };

    // [A, x, B, y, C]: move C to the front, drop x, replace y, move A to the end.
    let (doc, list, a, b, c) = setup()?;
    let _ = b;
    apply_then_undo_redo(
        &doc,
        list.id(),
        vec![
            ins(vec![cid(c.id()), val("n1")]),
            del(2),
            retain(1),
            del(1),
            ins(vec![val("n2")]),
            del(1),
            ins(vec![cid(a.id())]),
        ],
        json!({"l": ["C", "n1", "B", "n2", "A"]}),
    )?;

    // Children moved right past retained scalars end up after them.
    let (doc, list, a, b, c) = setup()?;
    apply_then_undo_redo(
        &doc,
        list.id(),
        vec![
            del(1),
            retain(1),
            del(1),
            retain(1),
            del(1),
            ins(vec![cid(c.id()), cid(b.id()), cid(a.id())]),
        ],
        json!({"l": ["x", "y", "C", "B", "A"]}),
    )?;

    // Children moved left are inserted before their own delete.
    let (doc, list, _a, b, c) = setup()?;
    apply_then_undo_redo(
        &doc,
        list.id(),
        vec![
            ins(vec![cid(c.id())]),
            retain(1),
            ins(vec![cid(b.id())]),
            retain(1),
            del(1),
            retain(1),
            del(1),
        ],
        json!({"l": ["C", "A", "B", "x", "y"]}),
    )?;

    // Mixed: one child moves left, one right, one stays, scalars are
    // deleted and inserted around them.
    let (doc, list, a, b, c) = setup()?;
    apply_then_undo_redo(
        &doc,
        list.id(),
        vec![
            del(1),
            ins(vec![val("n0"), cid(c.id())]),
            retain(2),
            del(2),
            ins(vec![val("n1"), cid(a.id()), val("n2")]),
        ],
        json!({"l": ["n0", "C", "x", "B", "n1", "A", "n2"]}),
    )?;
    assert_eq!(list.len(), 7);
    let _ = b;
    Ok(())
}

#[test]
fn apply_diff_keeps_child_content_edited_in_the_same_batch() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let (list, t) = text_list(&doc, &["A", "B"])?;
    let mut batch = DiffBatch::default();
    batch
        .push(
            list.id(),
            Diff::List(vec![del(2), ins(vec![cid(t[1].id()), cid(t[0].id())])]),
        )
        .unwrap();
    batch
        .push(
            t[0].id(),
            Diff::Text(vec![
                TextDelta::Retain {
                    retain: 1,
                    attributes: None,
                },
                TextDelta::Insert {
                    insert: "!".into(),
                    attributes: None,
                },
            ]),
        )
        .unwrap();
    doc.apply_diff(batch)?;
    doc.commit();
    assert_eq!(deep_json(&doc), json!({"l": ["B", "A!"]}));
    Ok(())
}

/// A delta with several adjacent delete items must behave like one merged
/// delete (loro.js review round 3, B2).
#[test]
fn apply_diff_handles_adjacent_delete_items() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let list = doc.get_movable_list("l");
    list.push("X")?;
    list.push("Y")?;
    let t = list.insert_container(2, LoroText::new())?;
    t.insert(0, "T")?;
    list.push("tail")?;
    doc.commit();
    apply_then_undo_redo(
        &doc,
        list.id(),
        vec![
            del(1),
            del(2),
            ins(vec![val("new")]),
            retain(1),
            ins(vec![cid(t.id())]),
        ],
        json!({"l": ["new", "tail", "T"]}),
    )?;

    // The Rust diff of `set(0); move(2, 3); move(1, 4)` starts with
    // `delete 1, delete 2, insert [set0]`.
    let src = LoroDoc::new();
    src.set_peer_id(1)?;
    let list = src.get_movable_list("l");
    for i in 0..5 {
        list.push(i)?;
    }
    src.commit();
    let v0 = src.state_frontiers();
    list.set(0, "set0")?;
    list.mov(2, 3)?;
    list.mov(1, 4)?;
    src.commit();
    let receiver = src.fork_at(&v0)?;
    receiver.apply_diff(src.diff(&v0, &src.state_frontiers())?)?;
    assert_eq!(deep_json(&receiver), deep_json(&src));
    Ok(())
}

/// Before the fix, undo after applying this diff panicked with an index
/// underflow (`OutOfBound` in release builds) in the movable list handler.
#[test]
fn undo_after_applying_a_diff_of_cancelling_moves() -> LoroResult<()> {
    let src = LoroDoc::new();
    src.set_peer_id(1)?;
    let list = src.get_movable_list("l");
    list.insert_container(0, LoroText::new())?.insert(0, "t1")?;
    list.insert_container(0, LoroText::new())?.insert(0, "t2")?;
    list.set(1, "s")?;
    src.commit();
    let v0 = src.state_frontiers();
    list.mov(0, 1)?;
    list.mov(0, 1)?;
    src.commit();
    let v1 = src.state_frontiers();

    let receiver = src.fork_at(&v0)?;
    receiver.set_peer_id(2)?;
    let mut undo = UndoManager::new(&receiver);
    undo.set_merge_interval(0);
    receiver.apply_diff(src.diff(&v0, &v1)?)?;
    receiver.commit();
    assert_eq!(deep_json(&receiver), json!({"l": ["t2", "s"]}));
    assert!(undo.undo()?);
    assert_eq!(deep_json(&receiver), json!({"l": ["t2", "s"]}));
    assert!(undo.redo()?);
    assert_eq!(deep_json(&receiver), json!({"l": ["t2", "s"]}));
    Ok(())
}

fn movable_list(doc: &LoroDoc, nested: bool) -> LoroMovableList {
    if nested {
        doc.get_map("root")
            .ensure_mergeable_movable_list("l")
            .unwrap()
    } else {
        doc.get_movable_list("l")
    }
}

fn random_list_op(rng: &mut StdRng, list: &LoroMovableList, n: &mut i64) {
    let len = list.len();
    *n += 1;
    let n = *n;
    match rng.gen_range(0..8) {
        0 => list.insert(rng.gen_range(0..=len), n).unwrap(),
        1 => {
            let t = list
                .insert_container(rng.gen_range(0..=len), LoroText::new())
                .unwrap();
            t.insert(0, &format!("t{n}")).unwrap();
        }
        2 => {
            let m = list
                .insert_container(rng.gen_range(0..=len), LoroMap::new())
                .unwrap();
            m.insert("k", n).unwrap();
        }
        3 if len > 0 => {
            let pos = rng.gen_range(0..len);
            let del_len = rng.gen_range(1..=2.min(len - pos));
            list.delete(pos, del_len).unwrap();
        }
        4 if len > 1 => list
            .mov(rng.gen_range(0..len), rng.gen_range(0..len))
            .unwrap(),
        5 if len > 0 => list.set(rng.gen_range(0..len), format!("s{n}")).unwrap(),
        6 if len > 0 => {
            let m = list
                .set_container(rng.gen_range(0..len), LoroMap::new())
                .unwrap();
            m.insert("set", n).unwrap();
        }
        7 if len > 0 => {
            if let Some(ValueOrContainer::Container(loro::Container::Text(t))) =
                list.get(rng.gen_range(0..len))
            {
                t.insert(0, "+").unwrap();
            }
        }
        _ => list.push(n).unwrap(),
    }
}

/// Differential test against the Rust diff: for random movable list histories,
/// `apply_diff(diff(a, b))` on a fork at `a` must produce `b`, and undo/redo
/// must switch between `a` and `b`.
#[test]
fn random_history_diff_apply_undo_redo() -> LoroResult<()> {
    for seed in 0..64u64 {
        for nested in [false, true] {
            let mut rng = StdRng::seed_from_u64(seed);
            let src = LoroDoc::new();
            src.set_peer_id(1)?;
            let list = movable_list(&src, nested);
            let mut n = 0;
            for _ in 0..4 {
                random_list_op(&mut rng, &list, &mut n);
            }
            src.commit();
            for round in 0..4 {
                let a = src.state_frontiers();
                let before = deep_json(&src);
                for _ in 0..8 {
                    random_list_op(&mut rng, &list, &mut n);
                }
                src.commit();
                let b = src.state_frontiers();
                let after = deep_json(&src);

                let receiver = src.fork_at(&a)?;
                receiver.set_peer_id(2)?;
                let mut undo = UndoManager::new(&receiver);
                undo.set_merge_interval(0);
                receiver.apply_diff(src.diff(&a, &b)?)?;
                receiver.commit();
                let ctx = format!("seed={seed} nested={nested} round={round}");
                assert_eq!(deep_json(&receiver), after, "apply {ctx}");
                undo.undo()?;
                assert_eq!(deep_json(&receiver), before, "undo {ctx}");
                undo.redo()?;
                assert_eq!(deep_json(&receiver), after, "redo {ctx}");
            }
        }
    }
    Ok(())
}

/// Builds a random list delta for `list`: every element is retained, deleted,
/// or (for children) moved to a random gap; new scalars and new Text children
/// are inserted into random gaps. Delete and insert items are randomly split.
/// Returns the diff batch and the expected list value.
fn random_list_delta(
    rng: &mut StdRng,
    doc: &LoroDoc,
    list: &LoroMovableList,
    fresh: &mut u32,
) -> (DiffBatch, Value) {
    #[derive(Clone)]
    enum Entry {
        Scalar(LoroValue),
        Child(ContainerID, Value),
        NewText(ContainerID, String),
    }

    let current = list.get_value();
    let current = current.as_list().unwrap();
    let deep = list.get_deep_value().to_json_value();
    let len = current.len();
    let mut keep = vec![false; len];
    let mut gaps: Vec<Vec<(Entry, bool)>> = vec![Vec::new(); len + 1];
    let mut edited_child = None;
    for i in 0..len {
        match (&current[i], rng.gen_range(0..10)) {
            (_, 0..=3) => keep[i] = true,
            (LoroValue::Container(id), 4..=7) => {
                let mut value = deep[i].clone();
                if id.container_type() == ContainerType::Text
                    && edited_child.is_none()
                    && rng.gen_bool(0.3)
                {
                    edited_child = Some(id.clone());
                    value = json!(format!("{}!", value.as_str().unwrap()));
                }
                let gap = rng.gen_range(0..=len);
                gaps[gap].push((Entry::Child(id.clone(), value), rng.gen_bool(0.5)));
            }
            _ => {}
        }
    }
    for _ in 0..rng.gen_range(0..4) {
        *fresh += 1;
        let entry = if rng.gen_bool(0.5) {
            Entry::Scalar(LoroValue::from(format!("v{fresh}")))
        } else {
            Entry::NewText(
                ContainerID::new_normal(ID::new(1000, *fresh as i32), ContainerType::Text),
                format!("n{fresh}"),
            )
        };
        gaps[rng.gen_range(0..=len)].push((entry, false));
    }

    let mut items: Vec<ListDiffItem> = Vec::new();
    let mut expected = Vec::new();
    let mut new_texts = Vec::new();
    for (gap, entries) in gaps.iter_mut().enumerate() {
        entries.shuffle(rng);
        let mut chunk: Vec<ValueOrContainer> = Vec::new();
        let mut chunk_is_move = false;
        for (entry, is_move) in entries.drain(..) {
            if !chunk.is_empty() && (rng.gen_bool(0.3) || is_move != chunk_is_move) {
                items.push(ListDiffItem::Insert {
                    insert: std::mem::take(&mut chunk),
                    is_move: chunk_is_move,
                });
            }
            chunk_is_move = is_move;
            match entry {
                Entry::Scalar(v) => {
                    expected.push(v.to_json_value());
                    chunk.push(val(v));
                }
                Entry::Child(id, value) => {
                    expected.push(value);
                    chunk.push(cid(id));
                }
                Entry::NewText(id, s) => {
                    expected.push(json!(s));
                    chunk.push(cid(id.clone()));
                    new_texts.push((id, s));
                }
            }
        }
        if !chunk.is_empty() {
            items.push(ListDiffItem::Insert {
                insert: chunk,
                is_move: chunk_is_move,
            });
        }
        if gap < len {
            if keep[gap] {
                expected.push(deep[gap].clone());
                match items.last_mut() {
                    Some(ListDiffItem::Retain { retain }) => *retain += 1,
                    _ => items.push(retain(1)),
                }
            } else {
                match items.last_mut() {
                    Some(ListDiffItem::Delete { delete }) if rng.gen_bool(0.5) => *delete += 1,
                    _ => items.push(del(1)),
                }
            }
        }
    }

    let mut batch = DiffBatch::default();
    batch.push(list.id(), Diff::List(items)).unwrap();
    for (id, s) in new_texts {
        batch
            .push(
                id,
                Diff::Text(vec![TextDelta::Insert {
                    insert: s,
                    attributes: None,
                }]),
            )
            .unwrap();
    }
    if let Some(id) = edited_child {
        let text = doc.get_text(id.clone());
        batch
            .push(
                id,
                Diff::Text(vec![
                    TextDelta::Retain {
                        retain: text.len_unicode(),
                        attributes: None,
                    },
                    TextDelta::Insert {
                        insert: "!".into(),
                        attributes: None,
                    },
                ]),
            )
            .unwrap();
    }
    (batch, Value::Array(expected))
}

/// Random deltas with multi-child moves, mixed scalars, and split delete items.
/// Each applied delta must produce the expected value, and undoing/redoing the
/// whole sequence must walk back and forth through every intermediate value.
#[test]
fn random_delta_apply_undo_redo() -> LoroResult<()> {
    for seed in 0..128u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let nested = seed % 2 == 1;
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let list = movable_list(&doc, nested);
        let mut n = 0;
        for _ in 0..rng.gen_range(2..10) {
            random_list_op(&mut rng, &list, &mut n);
        }
        doc.commit();

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let mut history = vec![deep_json(&doc)];
        let mut fresh = 0;
        for step in 0..6 {
            let version = doc.state_frontiers();
            if rng.gen_bool(0.3) {
                random_list_op(&mut rng, &list, &mut n);
            } else {
                let (batch, expected) = random_list_delta(&mut rng, &doc, &list, &mut fresh);
                doc.apply_diff(batch)?;
                assert_eq!(
                    list.get_deep_value().to_json_value(),
                    expected,
                    "seed={seed} step={step}"
                );
            }
            doc.commit();
            // A no-op delta creates no change and no undo step
            if doc.state_frontiers() != version {
                history.push(deep_json(&doc));
            }
        }

        for expected in history.iter().rev().skip(1) {
            assert!(undo.undo()?, "seed={seed}");
            assert_eq!(&deep_json(&doc), expected, "undo seed={seed}");
        }
        assert!(!undo.can_undo());
        for expected in history.iter().skip(1) {
            assert!(undo.redo()?, "seed={seed}");
            assert_eq!(&deep_json(&doc), expected, "redo seed={seed}");
        }
    }
    Ok(())
}

/// Two peers mix local ops, revert-style `apply_diff`, sync, and undo/redo.
/// Undo is not expected to restore an exact value under concurrency, but it
/// must not panic or fail, and the peers must converge.
#[test]
fn random_concurrent_apply_diff_undo_redo_does_not_panic() -> LoroResult<()> {
    for seed in 0..48u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let docs = [LoroDoc::new(), LoroDoc::new()];
        docs[0].set_peer_id(1)?;
        docs[1].set_peer_id(2)?;
        let mut n = 0;
        {
            let list = movable_list(&docs[0], false);
            for _ in 0..6 {
                random_list_op(&mut rng, &list, &mut n);
            }
            docs[0].commit();
            docs[1].import(&docs[0].export(ExportMode::all_updates())?)?;
        }
        let mut undos = [UndoManager::new(&docs[0]), UndoManager::new(&docs[1])];
        undos[0].set_merge_interval(0);
        undos[1].set_merge_interval(0);
        let mut versions = [
            vec![docs[0].state_frontiers()],
            vec![docs[1].state_frontiers()],
        ];

        for _ in 0..40 {
            let p = rng.gen_range(0..2);
            let doc = &docs[p];
            match rng.gen_range(0..10) {
                0..=3 => {
                    let list = movable_list(doc, false);
                    random_list_op(&mut rng, &list, &mut n);
                    doc.commit();
                }
                4 | 5 => {
                    let target = versions[p].choose(&mut rng).unwrap().clone();
                    let diff = doc.diff(&doc.state_frontiers(), &target)?;
                    doc.apply_diff(diff)?;
                    doc.commit();
                }
                6 => {
                    undos[p].undo()?;
                }
                7 => {
                    undos[p].redo()?;
                }
                _ => {
                    let other = &docs[1 - p];
                    doc.import(&other.export(ExportMode::updates(&doc.oplog_vv()))?)?;
                }
            }
            versions[p].push(doc.state_frontiers());
        }

        docs[1].import(&docs[0].export(ExportMode::all_updates())?)?;
        docs[0].import(&docs[1].export(ExportMode::all_updates())?)?;
        assert_eq!(deep_json(&docs[0]), deep_json(&docs[1]), "seed={seed}");
    }
    Ok(())
}
