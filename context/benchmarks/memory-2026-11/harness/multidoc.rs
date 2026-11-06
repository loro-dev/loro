//! Fixed per-doc overhead. usage: multidoc <mode> <n> [snapshot]
use loro::{ExportMode, LoroDoc, LoroMap, LoroText};
use loro_mem::*;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
fn small(body: usize, turns: usize) -> Vec<u8> {
    let d = LoroDoc::new();
    d.set_peer_id(1).unwrap();
    let h = d.get_list("history");
    for i in 0..turns {
        let m = h.push_container(LoroMap::new()).unwrap();
        m.insert("id", format!("turn-{i}")).unwrap();
        m.insert_container("text", LoroText::new()).unwrap().insert(0, &"x y ".repeat(body / 4)).unwrap();
        d.commit();
    }
    d.export(ExportMode::Snapshot).unwrap()
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mode, n) = (a[1].as_str(), a[2].parse::<usize>().unwrap());
    #[cfg(feature = "dhat_heap")]
    let prof = dhat::Profiler::builder().file_name(std::env::var("DHAT_OUT").unwrap()).trim_backtraces(Some(48)).build();
    let snap: Vec<u8> = match a.get(3).map(|s| s.as_str()) {
        Some("tiny") => small(100, 1),
        Some("s10") => small(400, 10),
        Some("s100") => small(400, 100),
        Some("s1000") => small(400, 1000),
        Some(p) => std::fs::read(p).unwrap(),
        None => vec![],
    };
    trim();
    let shared = bytes::Bytes::from(snap.clone());
    let (l0, b0, r0) = (LIVE.load(Relaxed), LIVE_BLOCKS.load(Relaxed), proc_kb("RssAnon"));
    let mut docs = Vec::with_capacity(n);
    let mut subs = vec![];
    for _ in 0..n {
        let d = match mode {
            "new" | "newsub" | "newwrite" => LoroDoc::new(),
            "open" | "openread" | "opensub" | "openwrite" => LoroDoc::from_snapshot(&snap).unwrap(),
            #[cfg(feature = "main_api")]
            "open0" | "open0read" | "open0write" => LoroDoc::from_snapshot_bytes(shared.clone()).unwrap(),
            _ => panic!(),
        };
        if mode.ends_with("sub") {
            subs.push(d.subscribe_root(Arc::new(|_| {})));
        }
        if mode.ends_with("read") {
            let l = d.get_list("history");
            let _ = l.get(l.len() - 1);
        }
        if mode.ends_with("write") {
            d.set_peer_id(1).unwrap();
            let t = d.get_text("t");
            t.insert(0, "hello").unwrap();
            d.commit();
        }
        docs.push(d);
    }
    let (l1, b1, r1) = (LIVE.load(Relaxed), LIVE_BLOCKS.load(Relaxed), proc_kb("RssAnon"));
    println!("mode={mode} n={n} snapshot={}B live/doc={:.0}B blocks/doc={:.1} rss_anon/doc={:.0}B total_live={:.1}MB total_anon={:.1}MB", snap.len(), (l1 - l0) as f64 / n as f64, (b1 - b0) as f64 / n as f64, (r1 as f64 - r0 as f64) * 1024.0 / n as f64, mb(l1), r1 as f64 / 1024.0);
    drop(shared);
    #[cfg(feature = "dhat_heap")]
    drop(prof);
    std::process::exit(0);
}
