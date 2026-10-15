//! Cost of model operations vs. document size.
//!
//! cargo run --release -p loro-model --example model_bench

#[path = "../tests/common/mod.rs"]
mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use loro::{ExportMode, LoroDoc};
use loro_model::View;

fn avg(iters: u32, mut f: impl FnMut(u32)) -> Duration {
    let start = Instant::now();
    for i in 0..iters {
        f(i);
    }
    start.elapsed() / iters
}

fn fmt(d: Duration) -> String {
    let ns = d.as_nanos() as f64;
    if ns < 1e3 {
        format!("{ns:.0} ns")
    } else if ns < 1e6 {
        format!("{:.1} µs", ns / 1e3)
    } else {
        format!("{:.1} ms", ns / 1e6)
    }
}

fn project(n: usize) -> Project {
    Project {
        title: "bench".into(),
        todos: (0..n)
            .map(|i| todo(&format!("todo-{i}"), "title"))
            .collect(),
        notes: "notes".into(),
    }
}

fn main() {
    let sizes = [1_000usize, 10_000, 100_000];
    let mut rows: Vec<(&str, Vec<String>)> = Vec::new();
    let mut push = |name: &'static str, col: usize, v: String| {
        if let Some(r) = rows.iter_mut().find(|r| r.0 == name) {
            r.1[col] = v;
        } else {
            let mut cells = vec![String::new(); sizes.len()];
            cells[col] = v;
            rows.push((name, cells));
        }
    };

    for (col, &n) in sizes.iter().enumerate() {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let model = ProjectModel::attach(doc.get_map("project"));
        let mut p = project(n);

        let t = Instant::now();
        model.update_from(&p).unwrap();
        doc.commit();
        push("update_from: initial write", col, fmt(t.elapsed()));

        // Baselines: the same data written with raw handlers, regular child
        // containers (list of maps) and mergeable children (map of maps).
        {
            let raw = LoroDoc::new();
            let list = raw
                .get_map("project")
                .insert_container("todos", loro::LoroMovableList::new())
                .unwrap();
            let t = Instant::now();
            for (i, td) in p.todos.iter().enumerate() {
                let m = list.insert_container(i, loro::LoroMap::new()).unwrap();
                m.insert("id", td.id.as_str()).unwrap();
                m.insert("title", td.title.as_str()).unwrap();
                m.insert("done", td.done).unwrap();
            }
            raw.commit();
            push("baseline: raw write, list of maps", col, fmt(t.elapsed()));
            let t = Instant::now();
            let deep = raw.get_map("project").get_deep_value();
            push(
                "baseline: get_deep_value(), list of maps",
                col,
                fmt(t.elapsed()),
            );
            drop(deep);
            let raw2 = LoroDoc::new();
            let items = raw2
                .get_map("project")
                .ensure_mergeable_map("items")
                .unwrap();
            let t = Instant::now();
            for td in p.todos.iter() {
                let m = items.ensure_mergeable_map(&td.id).unwrap();
                m.insert("id", td.id.as_str()).unwrap();
                m.insert("title", td.title.as_str()).unwrap();
                m.insert("done", td.done).unwrap();
            }
            raw2.commit();
            push(
                "baseline: raw write, mergeable map of maps",
                col,
                fmt(t.elapsed()),
            );
            let t = Instant::now();
            let deep = raw2.get_map("project").get_deep_value();
            push(
                "baseline: get_deep_value(), mergeable map of maps",
                col,
                fmt(t.elapsed()),
            );
            drop(deep);
        }

        let t = Instant::now();
        let deep = doc.get_map("project").get_deep_value();
        push("get_deep_value() of the model root", col, fmt(t.elapsed()));
        drop(deep);

        let t = Instant::now();
        let read: Project = model.read_as().unwrap();
        push("read_as::<Project>()", col, fmt(t.elapsed()));
        assert_eq!(read.todos.len(), n);

        let t = Instant::now();
        model.update_from(&p).unwrap();
        push("update_from: unchanged (0 ops)", col, fmt(t.elapsed()));

        p.todos[n / 2].title = "changed".into();
        let t = Instant::now();
        model.update_from(&p).unwrap();
        doc.commit();
        push("update_from: 1 field changed", col, fmt(t.elapsed()));

        let last = p.todos.pop().unwrap();
        p.todos.insert(0, last);
        let t = Instant::now();
        model.update_from(&p).unwrap();
        doc.commit();
        push("update_from: move last to front", col, fmt(t.elapsed()));

        push(
            "view: title().get()",
            col,
            fmt(avg(10_000, |_| {
                model.title().get().unwrap();
            })),
        );
        push(
            "view: title().set() + commit",
            col,
            fmt(avg(1_000, |i| {
                model.title().set(format!("t{i}")).unwrap();
                doc.commit();
            })),
        );

        let first_key = p.todos[0].id.clone();
        let last_key = p.todos[n - 1].id.clone();
        push(
            "view: by_key(first)",
            col,
            fmt(avg(1_000, |_| {
                model.todos().by_key(first_key.as_str()).unwrap().unwrap();
            })),
        );
        let iters = (2_000_000 / n as u32).max(5);
        push(
            "view: by_key(last)",
            col,
            fmt(avg(iters, |_| {
                model.todos().by_key(last_key.as_str()).unwrap().unwrap();
            })),
        );
        let held = model.todos().by_key(last_key.as_str()).unwrap().unwrap();
        push(
            "view: held element .done().set() + commit",
            col,
            fmt(avg(1_000, |i| {
                held.done().set(i % 2 == 0).unwrap();
                doc.commit();
            })),
        );
        push(
            "view: held element .title().get()",
            col,
            fmt(avg(10_000, |_| {
                held.title().get().unwrap();
            })),
        );

        // Remote side: import a one-field change with and without a watcher.
        let bob = LoroDoc::new();
        bob.import(&doc.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        let bob_model = ProjectModel::attach(bob.get_map("project"));
        let bob_held = bob_model
            .todos()
            .by_key(last_key.as_str())
            .unwrap()
            .unwrap();
        let updates: Vec<Vec<u8>> = (0..200)
            .map(|i| {
                let vv = doc.oplog_vv();
                held.title().set(format!("remote {i}")).unwrap();
                doc.commit();
                doc.export(ExportMode::updates(&vv)).unwrap()
            })
            .collect();
        bob.import(&updates[0]).unwrap(); // warm-up
        push(
            "remote: import 1-field update",
            col,
            fmt(avg(99, |i| {
                bob.import(&updates[1 + i as usize]).unwrap();
            })),
        );
        let hits = Arc::new(Mutex::new(0usize));
        let h = hits.clone();
        let _w = bob_held
            .title()
            .watch(move |_: &String, _: &String| *h.lock().unwrap() += 1)
            .unwrap();
        push(
            "remote: import 1-field update + watch(old,new)",
            col,
            fmt(avg(100, |i| {
                bob.import(&updates[100 + i as usize]).unwrap();
            })),
        );
        assert_eq!(*hits.lock().unwrap(), 100);
        assert_eq!(bob_held.title().get().unwrap(), "remote 199");
    }

    let width = rows.iter().map(|r| r.0.len()).max().unwrap();
    print!("| {:width$} |", "operation");
    for n in sizes {
        print!(" n = {n:>7} |");
    }
    println!();
    print!("|{}|", "-".repeat(width + 2));
    for _ in sizes {
        print!("-------------|");
    }
    println!();
    for (name, cells) in rows {
        print!("| {name:width$} |");
        for c in cells {
            print!(" {c:>11} |");
        }
        println!();
    }
}
