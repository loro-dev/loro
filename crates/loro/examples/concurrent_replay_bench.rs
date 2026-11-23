//! Reproduce #1327: cargo run -p loro --release --example concurrent_replay_bench
use loro::{ExportMode, LoroDoc};
use std::time::Instant;

fn main() {
    for peers in [
        [1, 2],
        [0x7000_0000_0000_0000, 0x7000_0000_0000_0001],
        [0xa5a5_a5a5_0000_0001, 0xa5a5_a5a5_0000_0002],
    ] {
        let docs = peers.map(|peer| {
            let doc = LoroDoc::new();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            for _ in 0..10_000 {
                text.insert(0, if peer == peers[0] { "a" } else { "b" })
                    .unwrap();
                doc.commit();
            }
            doc
        });
        let updates = docs
            .each_ref()
            .map(|d| d.export(ExportMode::all_updates()).unwrap());
        for i in 0..2 {
            let start = Instant::now();
            docs[i].import(&updates[1 - i]).unwrap();
            println!(
                "{:016x} <- {:016x}: {:.3} ms",
                peers[i],
                peers[1 - i],
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        assert_eq!(
            docs[0].get_text("text").to_string(),
            docs[1].get_text("text").to_string()
        );
        assert_eq!(
            docs[0].get_text("text").to_string(),
            "a".repeat(10_000) + &"b".repeat(10_000)
        );
    }
}
