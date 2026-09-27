//! Local `MovableList::set` + commit cost, one scenario per process
//! (loro-dev/loro#1124 review).
//!
//! ```sh
//! cargo run -p loro --release --example movable_set_bench -- cold   # fresh 20k list, first set pass
//! cargo run -p loro --release --example movable_set_bench -- seq    # 100k list, repeated set passes
//! ```
//!
//! `cold` builds a fresh document per sample (push 20k items + commit) and
//! times only the first pass of 20k `set` calls plus the commit; the document
//! is dropped after the timer stops. `seq` times 20 consecutive passes of 100k
//! `set` calls on one document and skips the first 2. Prints
//! `scenario,median_ms,samples...`.

use loro::LoroDoc;
use std::time::Instant;

fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn report(name: &str, samples: &[f64]) {
    let all: Vec<_> = samples.iter().map(|s| format!("{s:.3}")).collect();
    println!("{name},{:.3},{}", median(samples), all.join(","));
}

fn cold() {
    const N: usize = 20_000;
    const SAMPLES: usize = 15;
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let list = doc.get_movable_list("l");
        for _ in 0..N {
            list.push(0).unwrap();
        }
        doc.commit();
        let start = Instant::now();
        for i in 0..N {
            list.set(i, 1).unwrap();
        }
        doc.commit();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        drop(list);
        drop(doc);
    }
    report("movable_set_local_cold_20k", &samples);
}

fn seq() {
    const N: usize = 100_000;
    const ROUNDS: usize = 20;
    const SKIP: usize = 2;
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("l");
    for _ in 0..N {
        list.push(0).unwrap();
    }
    doc.commit();
    let mut samples = Vec::with_capacity(ROUNDS);
    for round in 0..ROUNDS {
        let start = Instant::now();
        for i in 0..N {
            list.set(i, round as i64 + 1).unwrap();
        }
        doc.commit();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    report("movable_set_local_seq_100k", &samples[SKIP..]);
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("cold") => cold(),
        Some("seq") => seq(),
        _ => panic!("usage: movable_set_bench cold|seq"),
    }
}
