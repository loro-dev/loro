use loro::{
    event::{Diff, DiffBatch, ListDiffItem},
    ContainerID, ContainerTrait, ContainerType, LoroDoc, LoroMovableList, LoroText, LoroValue,
    UndoManager, ValueOrContainer, ID,
};
use std::time::{Duration, Instant};

fn cid(id: ContainerID) -> ValueOrContainer {
    ValueOrContainer::Value(LoroValue::Container(id))
}

/// Builds `[children..., scalars...]` or `[scalars..., children...]`.
fn build(
    n: usize,
    children: usize,
    children_first: bool,
) -> (LoroDoc, LoroMovableList, Vec<ContainerID>) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("l");
    let mut ids = Vec::new();
    let push_children = |ids: &mut Vec<ContainerID>| {
        for i in 0..children {
            let t = list.push_container(LoroText::new()).unwrap();
            t.insert(0, &i.to_string()).unwrap();
            ids.push(t.id());
        }
    };
    if children_first {
        push_children(&mut ids);
    }
    for i in 0..n - children {
        list.push(i as i64).unwrap();
    }
    if !children_first {
        push_children(&mut ids);
    }
    doc.commit();
    (doc, list, ids)
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn bench(name: &str, n: usize, setup: impl Fn() -> (LoroDoc, LoroMovableList, Vec<ListDiffItem>)) {
    let mut apply = Vec::new();
    let mut undo = Vec::new();
    for _ in 0..5 {
        let (doc, list, items) = setup();
        let mut undo_manager = UndoManager::new(&doc);
        undo_manager.set_merge_interval(0);
        let mut batch = DiffBatch::default();
        batch.push(list.id(), Diff::List(items)).unwrap();
        let start = Instant::now();
        doc.apply_diff(batch).unwrap();
        doc.commit();
        apply.push(start.elapsed());
        let start = Instant::now();
        assert!(undo_manager.undo().unwrap());
        undo.push(start.elapsed());
    }
    println!(
        "{name:<24} n={n:<7} apply_diff={:>10.3?} undo={:>10.3?}",
        median(apply),
        median(undo)
    );
}

#[test]
#[ignore]
fn perf_movable_list_apply_diff() {
    // Run with:
    // cargo test --release -p loro --test perf_movable_list_apply_diff -- --ignored --nocapture
    let n: usize = std::env::var("LORO_PERF_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);

    bench("move-child-to-end", n, || {
        let (doc, list, ids) = build(n, 1, true);
        let items = vec![
            ListDiffItem::Delete { delete: 1 },
            ListDiffItem::Retain { retain: n - 1 },
            ListDiffItem::Insert {
                insert: vec![cid(ids[0].clone())],
                is_move: false,
            },
        ];
        (doc, list, items)
    });
    bench("move-child-to-front", n, || {
        let (doc, list, ids) = build(n, 1, false);
        let items = vec![
            ListDiffItem::Insert {
                insert: vec![cid(ids[0].clone())],
                is_move: false,
            },
            ListDiffItem::Retain { retain: n - 1 },
            ListDiffItem::Delete { delete: 1 },
        ];
        (doc, list, items)
    });
    bench("insert-child-tail", n, || {
        let (doc, list, _) = build(n, 0, true);
        let new_id = ContainerID::new_normal(ID::new(9, 0), ContainerType::Text);
        let items = vec![
            ListDiffItem::Retain { retain: n },
            ListDiffItem::Insert {
                insert: vec![cid(new_id)],
                is_move: false,
            },
        ];
        (doc, list, items)
    });
    bench("delete-scalars", n, || {
        let (doc, list, _) = build(n, 0, true);
        (doc, list, vec![ListDiffItem::Delete { delete: n }])
    });
    for k in [1_000, 5_000] {
        bench(&format!("rotate-{k}-children"), n, || {
            let (doc, list, ids) = build(n, k, true);
            let mut rotated: Vec<_> = ids[1..].iter().cloned().map(cid).collect();
            rotated.push(cid(ids[0].clone()));
            let items = vec![
                ListDiffItem::Delete { delete: k },
                ListDiffItem::Insert {
                    insert: rotated,
                    is_move: false,
                },
            ];
            (doc, list, items)
        });
        bench(&format!("reverse-{k}-children"), n, || {
            let (doc, list, ids) = build(n, k, true);
            let items = vec![
                ListDiffItem::Delete { delete: k },
                ListDiffItem::Insert {
                    insert: ids.iter().rev().cloned().map(cid).collect(),
                    is_move: false,
                },
            ];
            (doc, list, items)
        });
    }
}
