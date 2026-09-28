//! Cost of the unknown-container pre-check in `_apply_diff`.
//!
//! `cargo test --release -p loro --test perf_unknown_container_apply_diff -- --ignored --nocapture`

use loro::{
    event::{Diff, DiffBatch, ListDiffItem},
    ContainerID, ContainerType, LoroDoc, LoroText, LoroValue, UndoManager, ValueOrContainer,
};
use std::time::{Duration, Instant};

const N: usize = 100_000;

fn median(mut v: Vec<Duration>) -> f64 {
    v.sort();
    v[v.len() / 2].as_secs_f64() * 1000.0
}

fn bench<S>(name: &str, setup: impl Fn() -> S, run: impl Fn(S)) {
    let times = (0..7)
        .map(|_| {
            let s = setup();
            let start = Instant::now();
            run(s);
            start.elapsed()
        })
        .collect();
    println!("PERF {name}: median {:.2} ms", median(times));
}

/// A movable list of `N` numbers with an unknown container in the middle.
/// Forged from a Text without ops so that older builds can import it too.
fn doc_with_unknown_element() -> LoroDoc {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let list = src.get_movable_list("ml");
    for i in 0..N {
        list.push(i as i64).unwrap();
    }
    let text = list.insert_container(N / 2, LoroText::new()).unwrap();
    src.commit();
    let cid = loro::ContainerTrait::id(&text).to_string();
    let json = serde_json::to_string(
        &src.export_json_updates_without_peer_compression(&Default::default(), &src.oplog_vv()),
    )
    .unwrap()
    .replace(&cid, &cid.replace(":Text", ":Unknown(9)"));
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.import_json_updates(json.as_str()).unwrap();
    doc
}

#[test]
#[ignore]
fn perf_unknown_element_move() {
    let base = doc_with_unknown_element();
    let moved = || {
        let doc = base.fork();
        let undo = UndoManager::new(&doc);
        let v0 = doc.state_frontiers();
        doc.get_movable_list("ml").mov(N / 2, 0).unwrap();
        doc.commit();
        let v1 = doc.state_frontiers();
        (doc, undo, v0, v1)
    };
    bench(
        "unknown elem move in 100k movable list: apply_diff(diff(v1, v0))",
        || {
            let (doc, _, v0, v1) = moved();
            let diff = doc.diff(&v1, &v0).unwrap();
            (doc, diff)
        },
        |(doc, diff)| doc.apply_diff(diff).unwrap(),
    );
    bench(
        "unknown elem move in 100k movable list: revert_to(v0)",
        moved,
        |(doc, _, v0, _)| doc.revert_to(&v0).unwrap(),
    );
    bench(
        "unknown elem move in 100k movable list: undo",
        moved,
        |(_, mut undo, _, _)| assert!(undo.undo().unwrap()),
    );
}

#[test]
#[ignore]
fn perf_plain_diffs() {
    let list_doc = {
        let doc = LoroDoc::new();
        let list = doc.get_list("l");
        for i in 0..N {
            list.push(i as i64).unwrap();
        }
        doc.commit();
        doc
    };
    bench(
        "apply_diff list 100k ints -> fresh doc",
        || {
            (
                LoroDoc::new(),
                list_doc
                    .diff(&Default::default(), &list_doc.state_frontiers())
                    .unwrap(),
            )
        },
        |(doc, diff)| doc.apply_diff(diff).unwrap(),
    );

    let map_doc = {
        let doc = LoroDoc::new();
        let map = doc.get_map("m");
        for i in 0..N {
            map.insert(&i.to_string(), i as i64).unwrap();
        }
        doc.commit();
        doc
    };
    bench(
        "apply_diff map 100k keys -> fresh doc",
        || {
            (
                LoroDoc::new(),
                map_doc
                    .diff(&Default::default(), &map_doc.state_frontiers())
                    .unwrap(),
            )
        },
        |(doc, diff)| doc.apply_diff(diff).unwrap(),
    );

    let children_doc = {
        let doc = LoroDoc::new();
        let list = doc.get_list("l");
        for i in 0..N / 10 {
            list.push_container(LoroText::new())
                .unwrap()
                .insert(0, &i.to_string())
                .unwrap();
        }
        doc.commit();
        doc
    };
    bench(
        "apply_diff list 10k child containers -> fresh doc",
        || {
            (
                LoroDoc::new(),
                children_doc
                    .diff(&Default::default(), &children_doc.state_frontiers())
                    .unwrap(),
            )
        },
        |(doc, diff)| doc.apply_diff(diff).unwrap(),
    );

    bench(
        "revert_to empty from 100k list",
        || list_doc.fork(),
        |doc| doc.revert_to(&Default::default()).unwrap(),
    );
    bench(
        "undo 100k list insert",
        || {
            let doc = LoroDoc::new();
            let undo = UndoManager::new(&doc);
            let list = doc.get_list("l");
            for i in 0..N {
                list.push(i as i64).unwrap();
            }
            doc.commit();
            (doc, undo)
        },
        |(_doc, mut undo)| assert!(undo.undo().unwrap()),
    );

    // The pre-check scans every value of a batch for unknown container ids
    // before it takes the fast path
    let ml = ContainerID::new_root("ml", ContainerType::MovableList);
    bench(
        "apply_diff 100k plain values into a movable list",
        || {
            let mut batch = DiffBatch::default();
            batch
                .push(
                    ml.clone(),
                    Diff::List(vec![ListDiffItem::Insert {
                        insert: (0..N)
                            .map(|i| ValueOrContainer::Value(LoroValue::from(i as i64)))
                            .collect(),
                        is_move: false,
                    }]),
                )
                .unwrap();
            (LoroDoc::new(), batch)
        },
        |(doc, batch)| doc.apply_diff(batch).unwrap(),
    );
}

/// A mergeable list `m.s` of `N` numbers, with an unknown element in the
/// middle if `unknown`, forged from a Text without ops in the typed JSON (a
/// string round trip would lose the mergeable marker).
fn doc_with_mergeable_list(unknown: bool) -> LoroDoc {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let list = src.get_map("m").ensure_mergeable_list("s").unwrap();
    for i in 0..N {
        list.push(i as i64).unwrap();
    }
    if unknown {
        list.insert_container(N / 2, LoroText::new()).unwrap();
    }
    src.commit();
    let mut json =
        src.export_json_updates_without_peer_compression(&Default::default(), &src.oplog_vv());
    for change in json.changes.iter_mut() {
        for op in change.ops.iter_mut() {
            if let loro::JsonOpContent::List(loro::JsonListOp::Insert { value, .. }) =
                &mut op.content
            {
                for v in value.iter_mut() {
                    if let LoroValue::Container(ContainerID::Normal { peer, counter, .. }) = v {
                        *v = LoroValue::Container(ContainerID::new_normal(
                            loro::ID::new(*peer, *counter),
                            ContainerType::Unknown(9),
                        ));
                    }
                }
            }
        }
    }
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.import_json_updates(json).unwrap();
    doc
}

#[test]
#[ignore]
fn perf_full_state_revival() {
    for unknown in [false, true] {
        let base = doc_with_mergeable_list(unknown);
        bench(
            &format!("full-state revive 100k mergeable list (unknown element: {unknown})"),
            || {
                let doc = base.fork();
                let target = doc.state_frontiers();
                doc.get_map("m").delete("s").unwrap();
                doc.commit();
                let diff = doc.diff(&doc.state_frontiers(), &target).unwrap();
                (doc, diff)
            },
            |(doc, diff)| doc.apply_diff(diff).unwrap(),
        );
    }
}

/// Replays `src` with the ids of its Text containers (which must have no ops)
/// turned into `Unknown(9)`, editing the typed JSON so mergeable markers stay.
fn forge_texts(src: &LoroDoc) -> LoroDoc {
    let mut json =
        src.export_json_updates_without_peer_compression(&Default::default(), &src.oplog_vv());
    let forge = |v: &mut LoroValue| {
        if let LoroValue::Container(ContainerID::Normal {
            peer,
            counter,
            container_type: ContainerType::Text,
        }) = v
        {
            *v = LoroValue::Container(ContainerID::new_normal(
                loro::ID::new(*peer, *counter),
                ContainerType::Unknown(9),
            ));
        }
    };
    for change in json.changes.iter_mut() {
        for op in change.ops.iter_mut() {
            match &mut op.content {
                loro::JsonOpContent::List(loro::JsonListOp::Insert { value, .. }) => {
                    value.iter_mut().for_each(forge)
                }
                loro::JsonOpContent::Map(loro::JsonMapOp::Insert { value, .. }) => forge(value),
                _ => {}
            }
        }
    }
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.import_json_updates(json).unwrap();
    doc
}

/// Many small full-state targets (from the second review of #1142): 2000
/// mergeable maps with 50 keys each, and one mergeable map with 100k keys,
/// optionally holding one unknown container each.
#[test]
#[ignore]
fn perf_many_full_state_targets() {
    for unknown in [false, true] {
        let src = LoroDoc::new();
        src.set_peer_id(1).unwrap();
        let m = src.get_map("m");
        for i in 0..2000 {
            let s = m.ensure_mergeable_map(&format!("s{i}")).unwrap();
            for k in 0..50 {
                s.insert(&format!("k{k}"), k as i64).unwrap();
            }
            if unknown {
                s.insert_container("u", LoroText::new()).unwrap();
            }
        }
        src.commit();
        let base = forge_texts(&src);
        let hide_all = |doc: &LoroDoc| {
            let m = doc.get_map("m");
            for i in 0..2000 {
                m.delete(&format!("s{i}")).unwrap();
            }
            doc.commit();
        };
        bench(
            &format!("full-state revive 2000 mergeable maps x50 keys (unknown: {unknown})"),
            || {
                let doc = base.fork();
                let target = doc.state_frontiers();
                hide_all(&doc);
                let diff = doc.diff(&doc.state_frontiers(), &target).unwrap();
                (doc, diff)
            },
            |(doc, diff)| doc.apply_diff(diff).unwrap(),
        );
        bench(
            &format!("undo of hiding 2000 mergeable maps (unknown: {unknown})"),
            || {
                let doc = base.fork();
                let undo = UndoManager::new(&doc);
                hide_all(&doc);
                (doc, undo)
            },
            |(_doc, mut undo)| assert!(undo.undo().unwrap()),
        );
    }
    for unknown in [false, true] {
        let src = LoroDoc::new();
        src.set_peer_id(1).unwrap();
        let s = src.get_map("m").ensure_mergeable_map("s").unwrap();
        for k in 0..100_000 {
            s.insert(&format!("k{k}"), k as i64).unwrap();
        }
        if unknown {
            s.insert_container("u", LoroText::new()).unwrap();
        }
        src.commit();
        let base = forge_texts(&src);
        bench(
            &format!("full-state revive mergeable map 100k keys (unknown: {unknown})"),
            || {
                let doc = base.fork();
                let target = doc.state_frontiers();
                doc.get_map("m").delete("s").unwrap();
                doc.commit();
                let diff = doc.diff(&doc.state_frontiers(), &target).unwrap();
                (doc, diff)
            },
            |(doc, diff)| doc.apply_diff(diff).unwrap(),
        );
    }
}
