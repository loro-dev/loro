//! Scaling probe: is the per-import cost of a concurrent (stale-head) import
//! proportional to the SIZE OF THE MAP, or to the size of the update?
//!
//! Env: BASE=<base notes> UPDATES=<chain len> PER=<maps per update>
use loro::{LoroDoc, LoroMap, LoroText, ValueOrContainer};
use std::time::Instant;

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let base_n = env("BASE", 2000);
    let updates = env("UPDATES", 400);
    let per = env("PER", 10);

    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    for i in 0..base_n {
        let m = base
            .get_map("notes")
            .insert_container(&format!("b{i}"), LoroMap::new())
            .unwrap();
        m.insert("t", "x").unwrap();
        m.insert_container("body", LoroText::new())
            .unwrap()
            .insert(0, "hi")
            .unwrap();
    }
    base.commit();
    let snapshot = base.export(loro::ExportMode::Snapshot).unwrap();

    // peer 2: one change the receiver has but peer 3 never sees
    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.import(&snapshot).unwrap();
    match p2.get_map("notes").get("b0").unwrap() {
        ValueOrContainer::Container(c) => c.into_map().unwrap().insert("t", "from-2").unwrap(),
        _ => unreachable!(),
    }
    p2.commit();
    let stale_head = p2
        .export(loro::ExportMode::updates(&base.oplog_vv()))
        .unwrap();

    // peer 3: a long linear chain on top of base alone
    let p3 = LoroDoc::new();
    p3.set_peer_id(3).unwrap();
    p3.import(&snapshot).unwrap();
    let mut chain = Vec::with_capacity(updates);
    for k in 0..updates {
        let from = p3.oplog_vv();
        for i in 0..per {
            let m = p3
                .get_map("notes")
                .insert_container(&format!("c{k}-{i}"), LoroMap::new())
                .unwrap();
            m.insert("t", "y").unwrap();
            m.insert_container("body", LoroText::new())
                .unwrap()
                .insert(0, "hello")
                .unwrap();
        }
        p3.commit();
        chain.push(p3.export(loro::ExportMode::updates(&from)).unwrap());
    }

    for with_stale in [false, true] {
        let r = LoroDoc::new();
        r.set_peer_id(9).unwrap();
        r.import(&snapshot).unwrap();
        if with_stale {
            r.import(&stale_head).unwrap();
        }
        let mut per_ms = Vec::with_capacity(chain.len());
        for u in &chain {
            let t = Instant::now();
            r.import(u).unwrap();
            per_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let total: f64 = per_ms.iter().sum();
        let at = |i: usize| per_ms.get(i).copied().unwrap_or(f64::NAN);
        println!(
            "BASE={base_n} UPDATES={updates} stale={with_stale:<5} #0 {:.2}ms #{} {:.2}ms #{} {:.2}ms  total {:.0}ms  heads={}",
            at(0),
            updates / 2,
            at(updates / 2),
            updates - 1,
            at(updates - 1),
            total,
            r.state_frontiers().len()
        );
    }
}
