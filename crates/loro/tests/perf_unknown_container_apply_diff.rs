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
