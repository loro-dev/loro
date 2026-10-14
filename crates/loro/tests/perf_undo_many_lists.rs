//! Undoing one commit that made many movable lists rebuilt each list's
//! tracker from the whole history. `values` is a control without lists.
//!
//! Measured (Apple M3 Pro, --release, load ~10), before -> after:
//!
//! | lists | values  | commit  | undo before | undo after | redo    |
//! |-------|---------|---------|-------------|------------|---------|
//! | 365   | 5,840   | 58 ms   | 91 ms       | 71 ms      | 63 ms   |
//! | 1,460 | 23,360  | 257 ms  | 585 ms      | 310 ms     | 312 ms  |
//! | 2,920 | 46,720  | 566 ms  | 2.1 s       | 691 ms     | 689 ms  |
//! | 5,840 | 93,440  | 1.4 s   | 9.5 s       | 1.5 s      | 1.5 s   |
//!
//! Run with:
//! cargo test -p loro --release perf_undo_of_a_commit_that_makes_many_lists -- --ignored --nocapture --test-threads=1
//!
//! Scale with:
//! LORO_PERF_LISTS=365,1460,5840 cargo test -p loro --release perf_undo_of_a_commit_that_makes_many_lists -- --ignored --nocapture --test-threads=1

use loro::{LoroDoc, LoroList, LoroMap, LoroMovableList, LoroValue, UndoManager};
use std::time::{Duration, Instant};

const PER_LIST: usize = 16;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn write_lists(meta: &LoroMap, lists: usize) {
    let paths = meta.insert_container("path", LoroList::new()).unwrap();
    for s in 0..lists {
        let sub = paths.push_container(LoroMap::new()).unwrap();
        sub.insert("closed", true).unwrap();
        let anchors = sub
            .insert_container("anchors", LoroMovableList::new())
            .unwrap();
        for k in 0..PER_LIST {
            let a = anchors.push_container(LoroMap::new()).unwrap();
            let (x, y) = (s as f64 * 10.0 + k as f64, k as f64 * 0.5);
            a.insert("id", (s * PER_LIST + k) as i64).unwrap();
            a.insert("x", x).unwrap();
            a.insert("y", y).unwrap();
            a.insert("in-x", x - 0.3).unwrap();
            a.insert("in-y", y - 0.3).unwrap();
            a.insert("out-x", x + 0.3).unwrap();
            a.insert("out-y", y + 0.3).unwrap();
        }
    }
}

fn write_values(meta: &LoroMap, lists: usize) {
    let map = meta.insert_container("path", LoroMap::new()).unwrap();
    for s in 0..lists {
        for k in 0..PER_LIST {
            let (x, y) = (s as f64 * 10.0 + k as f64, k as f64 * 0.5);
            let value: Vec<LoroValue> = vec![
                (s as i64).into(),
                x.into(),
                y.into(),
                (x - 0.3).into(),
                (y - 0.3).into(),
                (x + 0.3).into(),
                (y + 0.3).into(),
            ];
            map.insert(&(s * PER_LIST + k).to_string(), value).unwrap();
        }
    }
}

fn run(layout: &str, lists: usize) {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("scene");
    tree.enable_fractional_index(0);
    let page = tree.create(None).unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);

    let node = tree.create(page).unwrap();
    let meta = tree.get_meta(node).unwrap();
    let t = Instant::now();
    match layout {
        "values" => write_values(&meta, lists),
        _ => write_lists(&meta, lists),
    }
    doc.commit();
    let commit = t.elapsed();

    let t = Instant::now();
    assert!(undo.undo().unwrap());
    let undone = t.elapsed();
    assert!(tree.is_node_deleted(&node).unwrap());
    let t = Instant::now();
    assert!(undo.redo().unwrap());
    let redone = t.elapsed();
    println!(
        "{layout:6} {lists:>5} lists {:>6} values: commit {:>8.1} ms, undo {:>8.1} ms, redo {:>8.1} ms",
        lists * PER_LIST,
        ms(commit),
        ms(undone),
        ms(redone),
    );
}

#[test]
#[ignore]
fn perf_undo_of_a_commit_that_makes_many_lists() {
    let sizes: Vec<usize> = std::env::var("LORO_PERF_LISTS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().parse().unwrap()).collect())
        .unwrap_or_else(|| vec![365, 1460, 2920, 5840]);
    for &lists in &sizes {
        run("lists", lists);
    }
    for &lists in &sizes {
        run("values", lists);
    }
}
