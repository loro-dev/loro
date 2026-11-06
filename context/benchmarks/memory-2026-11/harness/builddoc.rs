//! Build synthetic docs with the same shape as loro-base-edit (chat history + long oplog).
use loro::{Container, ExportMode, LoroDoc, LoroList, LoroMap, LoroText, ValueOrContainer};
use rand::{rngs::SmallRng, Rng, SeedableRng};
const W: &[&str] = &["the","quick","memory","history","agent","tool","result","token","stream","update","file","patch","review","window","chat","message","context","error","state","value"];
fn words(r: &mut SmallRng, n: usize) -> String {
    let mut s = String::with_capacity(n + 8);
    while s.len() < n { s.push_str(W[r.gen_range(0..W.len())]); s.push(' '); }
    s.truncate(n);
    s
}
/// `chunk`: streamed-chunk size; one commit per turn, two peers.
pub fn chat(turns: usize, body: usize, chunk: usize, seed: u64) -> LoroDoc {
    let mut r = SmallRng::seed_from_u64(seed);
    let d = LoroDoc::new();
    let h: LoroList = d.get_list("history");
    for i in 0..turns {
        d.set_peer_id(1 + (i % 2) as u64).unwrap();
        let m = h.push_container(LoroMap::new()).unwrap();
        m.insert("id", format!("turn-{i}")).unwrap();
        m.insert("role", if i % 2 == 0 { "user" } else { "assistant" }).unwrap();
        m.insert("ts", 1_700_000_000i64 + i as i64).unwrap();
        let t = m.insert_container("text", LoroText::new()).unwrap();
        let (mut left, mut pos) = (body, 0);
        while left > 0 {
            let n = left.min(chunk);
            let w = words(&mut r, n);
            t.insert(pos, &w).unwrap();
            pos += w.chars().count();
            left -= n;
        }
        d.commit();
    }
    d
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = match a[1].as_str() {
        "chat" => chat(a[3].parse().unwrap(), a[4].parse().unwrap(), a[5].parse().unwrap(), 7),
        _ => panic!(),
    };
    let s = d.export(ExportMode::Snapshot).unwrap();
    std::fs::write(&a[2], &s).unwrap();
    let so = d.export(ExportMode::StateOnly(None)).unwrap();
    let h = d.get_list("history");
    let Some(ValueOrContainer::Container(Container::Map(_))) = h.get(0) else { panic!() };
    println!("{}: turns={} snapshot={:.2}MiB state_only={:.2}MiB ops={} changes={}", a[2], h.len(), s.len() as f64 / 1048576.0, so.len() as f64 / 1048576.0, d.len_ops(), d.len_changes());
}
