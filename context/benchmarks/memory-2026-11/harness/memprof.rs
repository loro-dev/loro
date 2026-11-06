//! Scripted stages over one or two docs; prints live heap (allocator-level) and RSS per stage.
//! usage: memprof <snapshot> <stage> <stage> ...   (env DHAT_OUT=path to stop & dump)
use loro::{Container, ContainerID, ContainerTrait, ExportMode, Frontiers, LoroDoc, LoroMap, LoroText, ValueOrContainer, VersionVector};
use loro_mem::*;
use rand::{rngs::SmallRng, Rng, SeedableRng};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Instant;

struct Ctx {
    snap: Option<Vec<u8>>,
    path: String,
    h: Option<LoroDoc>,
    s: Option<LoroDoc>,
    shared: Option<bytes::Bytes>,
    subs: Vec<loro::Subscription>,
    keep: Vec<Box<dyn std::any::Any>>,
    rng: SmallRng,
    peer: u64,
    base: Option<Frontiers>,
    upd: Option<Vec<u8>>,
    vv0: Option<VersionVector>,
}
fn map_at(h: &LoroDoc, i: usize) -> LoroMap {
    let l = h.get_list("history");
    let Some(ValueOrContainer::Container(Container::Map(m))) = l.get(i) else { panic!() };
    m
}
fn text_of(m: &LoroMap) -> LoroText {
    let Some(ValueOrContainer::Container(Container::Text(t))) = m.get("text") else { panic!() };
    t
}
fn last_text(h: &LoroDoc) -> ContainerID {
    text_of(&map_at(h, h.get_list("history").len() - 1)).id()
}
fn text(d: &LoroDoc, c: &ContainerID) -> LoroText {
    let Some(Container::Text(t)) = d.get_container(c.clone()) else { panic!() };
    t
}
fn batch(d: &LoroDoc, c: &ContainerID, pos: usize, i: usize) {
    text(d, c).insert(pos, "batched!").unwrap();
    let hist = d.get_list("history");
    let m = hist.push_container(LoroMap::new()).unwrap();
    m.insert("id", format!("b{i}")).unwrap();
    m.insert_container("text", LoroText::new()).unwrap().insert(0, "hello").unwrap();
    d.commit();
}
fn snap_bytes(c: &mut Ctx) -> Vec<u8> {
    c.snap.take().unwrap_or_else(|| std::fs::read(&c.path).unwrap())
}
/// Advance the head with `d` remote single-op edits on the last text.
fn remote(h: &LoroDoc, cid: &ContainerID, d: usize) {
    let t = text(h, cid);
    h.set_peer_id(2).unwrap();
    for _ in 0..d { t.push_str("r").unwrap(); }
    h.commit();
}
fn stage(c: &mut Ctx, st: &str) {
    let (name, arg) = st.split_once(':').unwrap_or((st, ""));
    let nums: Vec<usize> = arg.split(',').filter_map(|x| x.parse().ok()).collect();
    match name {
        "load" => { c.snap = Some(std::fs::read(&c.path).unwrap()); }
        "open" => {
            let b = snap_bytes(c);
            c.h = Some(LoroDoc::from_snapshot(&b).unwrap());
        }
        #[cfg(feature = "main_api")]
        "open0" => {
            let b = bytes::Bytes::from(snap_bytes(c));
            c.h = Some(LoroDoc::from_snapshot_bytes(b.clone()).unwrap());
            c.shared = Some(b);
        }
        #[cfg(feature = "main_api")]
        "openmmap" => {
            let f = std::fs::File::open(&c.path).unwrap();
            let m = unsafe { memmap2::Mmap::map(&f).unwrap() };
            let b = bytes::Bytes::from_owner(m);
            c.h = Some(LoroDoc::from_snapshot_bytes(b.clone()).unwrap());
            c.shared = Some(b);
        }
        "dropshared" => { c.shared = None; }
        "read_tail" => {
            let h = c.h.as_ref().unwrap();
            let n = h.get_list("history").len();
            let mut tot = 0;
            for i in n.saturating_sub(nums[0])..n { tot += text_of(&map_at(h, i)).to_string().len(); }
            println!("    read_tail bytes={tot}");
        }
        "read_at" => {
            let h = c.h.as_ref().unwrap();
            let n = h.get_list("history").len();
            let start = n / 2;
            let mut tot = 0;
            for i in start..(start + nums[0]).min(n) { tot += text_of(&map_at(h, i)).to_string().len(); }
            println!("    read_at bytes={tot}");
        }
        "read_all" => {
            let h = c.h.as_ref().unwrap();
            let n = h.get_list("history").len();
            let mut tot = 0;
            for i in 0..n { tot += text_of(&map_at(h, i)).len_unicode(); }
            println!("    read_all chars={tot}");
        }
        "len" => { println!("    list len={}", c.h.as_ref().unwrap().get_list("history").len()); }
        "append" => {
            let h = c.h.as_ref().unwrap();
            h.set_peer_id(1).unwrap();
            let cid = last_text(h);
            let t = text(h, &cid);
            for _ in 0..nums[0] { t.push_str("hello ").unwrap(); h.commit(); }
        }
        "push_turn" => {
            let h = c.h.as_ref().unwrap();
            h.set_peer_id(1).unwrap();
            let l = h.get_list("history");
            for i in 0..nums[0] {
                let m = l.push_container(LoroMap::new()).unwrap();
                m.insert("id", format!("n{i}")).unwrap();
                m.insert_container("text", LoroText::new()).unwrap().insert(0, "a short new message body of some sort").unwrap();
                h.commit();
            }
        }
        "alt_writes" => {
            let h = c.h.as_ref().unwrap();
            let cid = last_text(h);
            let t = text(h, &cid);
            for i in 0..nums[0] { h.set_peer_id(1 + (i % 2) as u64).unwrap(); t.push_str("x").unwrap(); h.commit(); }
        }
        "fresh_peer_writes" => {
            let h = c.h.as_ref().unwrap();
            let cid = last_text(h);
            let t = text(h, &cid);
            for _ in 0..nums[0] { c.peer += 1; h.set_peer_id(c.peer).unwrap(); t.push_str("x").unwrap(); h.commit(); }
        }
        "sub" => {
            let h = c.h.as_ref().unwrap();
            static N: AtomicU64 = AtomicU64::new(0);
            c.subs.push(h.subscribe_root(Arc::new(|e| { N.fetch_add(e.events.len() as u64, Relaxed); })));
        }
        "unsub" => { c.subs.clear(); }
        "deep" => {
            let v = c.h.as_ref().unwrap().get_deep_value();
            let s = serde_json::to_string(&v).unwrap();
            println!("    json bytes={}", s.len());
        }
        "deep_keep" => { let v = c.h.as_ref().unwrap().get_deep_value(); c.keep.push(Box::new(v)); }
        "droppkeep" => { c.keep.clear(); }
        "checkout_back" => {
            let h = c.h.as_ref().unwrap();
            let f = h.oplog_frontiers();
            let mut vv: VersionVector = h.oplog_vv();
            for (_, cnt) in vv.iter_mut() { *cnt = (*cnt - nums[0] as i32).max(0); }
            let old = h.vv_to_frontiers(&vv);
            c.base = Some(f);
            h.checkout(&old).unwrap();
        }
        "latest" => { c.h.as_ref().unwrap().checkout_to_latest(); }
        "attach" => { c.h.as_ref().unwrap().attach(); }
        "detach_edit" => { c.h.as_ref().unwrap().set_detached_editing(true); }
        "free_dc" => { for d in [&c.h, &c.s].into_iter().flatten() { d.free_diff_calculator(); } }
        "free_hc" => { for d in [&c.h, &c.s].into_iter().flatten() { d.free_history_cache(); } }
        "compact" => { for d in [&c.h, &c.s].into_iter().flatten() { d.compact_change_store(); } }
        "trim" => { trim(); }
        "export_snap" => { let b = c.h.as_ref().unwrap().export(ExportMode::Snapshot).unwrap(); println!("    snapshot={:.2}MiB", b.len() as f64 / 1048576.0); }
        "export_upd" => { let b = c.h.as_ref().unwrap().export(ExportMode::all_updates()).unwrap(); println!("    updates={:.2}MiB", b.len() as f64 / 1048576.0); }
        "cachelimit" => { c.h.as_ref().unwrap().set_decoded_value_cache_limit(nums[0]); }
        "shadow" => {
            let b = snap_bytes(c);
            let s = LoroDoc::from_snapshot(&b).unwrap();
            s.set_detached_editing(true);
            c.s = Some(s);
        }
        #[cfg(feature = "main_api")]
        "shadow0" => {
            let b = c.shared.clone().unwrap();
            let s = LoroDoc::from_snapshot_bytes(b).unwrap();
            s.set_detached_editing(true);
            c.s = Some(s);
        }
        "drop_shadow" => { c.s = None; }
        "drop_doc" => { c.h = None; }
        "w_detached" | "w_shadow" | "w_fork" => {
            let (d, n) = (nums[0], nums[1]);
            let free_each = nums.get(2).copied().unwrap_or(0) == 1;
            let h = c.h.clone().unwrap();
            let cid = last_text(&h);
            let mut lat = vec![];
            for i in 0..n {
                let base = h.oplog_frontiers();
                let bvv = h.oplog_vv();
                let tgt = text(&h, &cid);
                let pos = c.rng.gen_range(0..=tgt.len_unicode());
                remote(&h, &cid, d);
                c.peer += 1;
                let t = Instant::now();
                match name {
                    "w_detached" => {
                        h.checkout(&base).unwrap();
                        h.set_peer_id(c.peer).unwrap();
                        batch(&h, &cid, pos, i);
                        h.checkout_to_latest();
                        if free_each { h.free_diff_calculator(); }
                        #[cfg(feature = "proto")]
                        if nums.get(2).copied() == Some(2) { h.evict_parsed_blocks(); h.free_diff_calculator(); }
                    }
                    "w_fork" => {
                        let f = h.fork_at(&base).unwrap();
                        f.set_peer_id(c.peer).unwrap();
                        batch(&f, &cid, pos, i);
                        let u = f.export(ExportMode::updates(&bvv)).unwrap();
                        h.import(&u).unwrap();
                    }
                    _ => {
                        let s = c.s.as_ref().unwrap();
                        s.import(&h.export(ExportMode::updates(&s.oplog_vv())).unwrap()).unwrap();
                        s.checkout(&base).unwrap();
                        s.set_peer_id(c.peer).unwrap();
                        let before = s.oplog_vv();
                        batch(s, &cid, pos, i);
                        let u = s.export(ExportMode::updates(&before)).unwrap();
                        h.import(&u).unwrap();
                        if free_each { s.free_diff_calculator(); s.free_history_cache(); h.free_diff_calculator(); }
                        #[cfg(feature = "proto")]
                        if nums.get(2).copied() == Some(2) { s.evict_parsed_blocks(); s.free_diff_calculator(); s.free_history_cache(); h.evict_parsed_blocks(); }
                    }
                }
                lat.push(t.elapsed().as_secs_f64() * 1e3);
            }
            lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("    n={n} d={d} p50={:.2}ms max={:.2}ms", lat[n / 2], lat[n - 1]);
        }

        // ---- cold-open operation matrix ----
        "prep_upd" => { // a small sequential update + a concurrent one, built off to the side
            let b = std::fs::read(&c.path).unwrap();
            let o = LoroDoc::from_snapshot(&b).unwrap();
            o.set_peer_id(777).unwrap();
            let cid = last_text(&o);
            text(&o, &cid).push_str("from a remote peer").unwrap();
            o.commit();
            let vv0 = LoroDoc::from_snapshot(&b).unwrap().oplog_vv();
            c.upd = Some(o.export(ExportMode::updates(&vv0)).unwrap());
            c.vv0 = Some(vv0);
            drop(o);
            trim();
        }
        "import_upd" => { let st = c.h.as_ref().unwrap().import(c.upd.as_ref().unwrap()).unwrap(); println!("    pending={}", st.pending.is_some()); }
        "local_edit" => { // local edit that makes a later import concurrent
            let h = c.h.as_ref().unwrap();
            h.set_peer_id(888).unwrap();
            let cid = last_text(h);
            text(h, &cid).push_str("local").unwrap();
            h.commit();
        }
        "export_recent" => { let v = c.vv0.clone().unwrap_or_else(|| c.h.as_ref().unwrap().oplog_vv()); let b = c.h.as_ref().unwrap().export(ExportMode::updates(&v)).unwrap(); println!("    bytes={}", b.len()); }
        "f2v_head" => { let h = c.h.as_ref().unwrap(); let f = h.oplog_frontiers(); println!("    {}", h.frontiers_to_vv(&f).is_some()); }
        "v2f_head" => { let h = c.h.as_ref().unwrap(); let v = h.oplog_vv(); println!("    {}", h.vv_to_frontiers(&v).len()); }
        "fork" => { let f = c.h.as_ref().unwrap().fork(); c.keep.push(Box::new(f)); }
        "fork_at_head" => { let h = c.h.as_ref().unwrap(); let f = h.fork_at(&h.oplog_frontiers()).unwrap(); c.keep.push(Box::new(f)); }
        "diff_back" => {
            let h = c.h.as_ref().unwrap();
            let f = h.oplog_frontiers();
            let mut vv: VersionVector = h.oplog_vv();
            for (_, cnt) in vv.iter_mut() { *cnt = (*cnt - nums[0] as i32).max(0); }
            let old = h.vv_to_frontiers(&vv);
            println!("    diff containers={}", h.diff(&old, &f).unwrap().iter().count());
        }
        "cursor" => {
            let h = c.h.as_ref().unwrap();
            let cid = last_text(h);
            let t = text(h, &cid);
            let cur = t.get_cursor(1, loro::cursor::Side::Middle).unwrap();
            println!("    pos={}", h.get_cursor_pos(&cur).unwrap().current.pos);
        }
        "undo_setup" => { c.keep.push(Box::new(loro::UndoManager::new(c.h.as_ref().unwrap()))); }
        "shallow_at" => { // re-open a shallow snapshot rooted nums[0] ops behind head; replaces h
            let h = c.h.take().unwrap();
            let head = h.oplog_frontiers();
            let id = head.iter().next().unwrap();
            let f = Frontiers::from_id(loro::ID::new(id.peer, id.counter - nums[0] as i32));
            let b = h.export(ExportMode::shallow_snapshot(&f)).unwrap();
            println!("    shallow={:.2}MiB", b.len() as f64 / 1048576.0);
            drop(h);
            c.snap = Some(b);
        }
        "state_only" => {
            let h = c.h.take().unwrap();
            let b = h.export(ExportMode::StateOnly(None)).unwrap();
            println!("    state_only={:.2}MiB", b.len() as f64 / 1048576.0);
            drop(h);
            c.snap = Some(b);
        }
        #[cfg(feature = "proto")]
        "sizes" => { println!("    {}", LoroDoc::proto_sizes()); }
        #[cfg(feature = "proto")]
        "evict" => { for d in [&c.h, &c.s].into_iter().flatten() { println!("    evicted={}", d.evict_parsed_blocks()); } }
        "shallow_info" => { let h = c.h.as_ref().unwrap(); println!("    shallow={} since={:?} head={:?} head_vv={:?}", h.is_shallow(), h.shallow_since_frontiers(), h.oplog_frontiers(), h.oplog_vv()); }
        "shallow_small" => {
            // Why did a shallow export land near the first change? Vary each suspect alone.
            let build = |peers: u64, chunk: usize| {
                let d = LoroDoc::new();
                let l = d.get_list("history");
                for i in 0..200u64 {
                    d.set_peer_id(1 + i % peers).unwrap();
                    let m = l.push_container(LoroMap::new()).unwrap();
                    m.insert_container("text", LoroText::new()).unwrap().insert(0, &"x".repeat(chunk)).unwrap();
                    d.commit();
                }
                d
            };
            for (name, peers, cold) in [("1peer-warm", 1, false), ("2peer-warm", 2, false), ("1peer-cold", 1, true), ("2peer-cold", 2, true)] {
                let d = build(peers, 100);
                let d = if cold { LoroDoc::from_snapshot(&d.export(ExportMode::Snapshot).unwrap()).unwrap() } else { d };
                let id = d.oplog_frontiers().iter().next().unwrap();
                for back in [50, 500, 2000] {
                    let f = Frontiers::from_id(loro::ID::new(id.peer, id.counter - back));
                    let s = d.export(ExportMode::shallow_snapshot(&f)).unwrap();
                    let o = LoroDoc::from_snapshot(&s).unwrap();
                    println!("    {name} head={id:?} asked={f:?} since={:?} bytes={} changes={}", o.shallow_since_frontiers(), s.len(), o.len_changes());
                }
            }
            for name in ["read-back-1000", "real-doc"] {
                let h = c.h.as_ref().unwrap();
                let id = h.oplog_frontiers().iter().next().unwrap();
                let back = if name == "real-doc" { 20000 } else { 1000 };
                let f = Frontiers::from_id(loro::ID::new(id.peer, id.counter - back));
                let s = h.export(ExportMode::shallow_snapshot(&f)).unwrap();
                let o = LoroDoc::from_snapshot(&s).unwrap();
                println!("    {name} asked={f:?} since={:?} bytes={}", o.shallow_since_frontiers(), s.len());
            }
        }
        "stats" => {
            let h = c.h.as_ref().unwrap();
            println!("    ops={} changes={} peers={}", h.len_ops(), h.len_changes(), h.oplog_vv().len());
        }
        _ => panic!("unknown stage {st}"),
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    #[cfg(feature = "dhat_heap")]
    let prof = dhat::Profiler::builder().file_name(std::env::var("DHAT_OUT").unwrap()).trim_backtraces(Some(48)).build();
    let mut c = Ctx { snap: None, path: a[1].clone(), h: None, s: None, shared: None, subs: vec![], keep: vec![], rng: SmallRng::seed_from_u64(9), peer: 100_000, base: None, upd: None, vv0: None };
    println!("{:<26}{:>9}{:>10}{:>10}{:>11}{:>10}{:>10}{:>10}{:>12}", "stage", "ms", "live_MB", "delta_MB", "peak_MB", "blocks_k", "anon_MB", "file_MB", "allocs_k");
    let mut prev = LIVE.load(Relaxed);
    for st in &a[2..] {
        PEAK.store(LIVE.load(Relaxed), Relaxed);
        let a0 = ALLOCS.load(Relaxed);
        let t = Instant::now();
        stage(&mut c, st);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let live = LIVE.load(Relaxed);
        println!("{:<26}{:>9.1}{:>10.2}{:>+10.2}{:>11.2}{:>10.1}{:>10.1}{:>10.1}{:>12.0}", st, ms, mb(live), mb(live) - mb(prev), mb(PEAK.load(Relaxed)), LIVE_BLOCKS.load(Relaxed) as f64 / 1e3, proc_kb("RssAnon") as f64 / 1024.0, proc_kb("RssFile") as f64 / 1024.0, (ALLOCS.load(Relaxed) - a0) as f64 / 1e3);
        prev = live;
    }
    println!("peak_rss(VmHWM)={:.1}MB", proc_kb("VmHWM") as f64 / 1024.0);
    #[cfg(feature = "dhat_heap")]
    drop(prof);
    std::process::exit(0);
}
