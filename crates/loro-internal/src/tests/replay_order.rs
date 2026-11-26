use crate::{container::richtext::REPLAY_SCAN_STEPS, loro::ExportMode, LoroDoc};

fn check(peers: &[u64], shared_prefix: bool) {
    const N: usize = 1_000;
    let seed = LoroDoc::new_auto_commit();
    seed.set_peer_id(42).unwrap();
    seed.get_text("text").insert_unicode(0, "s").unwrap();
    seed.commit_then_renew();
    let seed = seed.export(ExportMode::all_updates()).unwrap();
    let docs: Vec<_> = peers
        .iter()
        .enumerate()
        .map(|(i, &peer)| {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            if shared_prefix && (peers.len() == 2 || i < 2) {
                doc.import(&seed).unwrap();
            }
            for _ in 0..N {
                doc.get_text("text")
                    .insert_unicode(0, ["a", "b", "c"][i])
                    .unwrap();
                doc.commit_then_renew();
            }
            doc
        })
        .collect();
    let updates: Vec<_> = docs
        .iter()
        .map(|d| d.export(ExportMode::all_updates()).unwrap())
        .collect();
    let mut order: Vec<_> = (0..peers.len()).collect();
    order.sort_unstable_by_key(|&i| peers[i]);
    let mut expected = String::new();
    for &i in &order {
        expected.push_str(&["a", "b", "c"][i].repeat(N));
        // The independent third branch is concurrent with the seed itself;
        // the seed's smaller peer puts its whole subtree before that branch.
        if shared_prefix && i == 1 {
            expected.push('s');
        }
    }
    for (i, doc) in docs.iter().enumerate() {
        for offset in 1..peers.len() {
            REPLAY_SCAN_STEPS.with(|c| c.set(0));
            doc.import(&updates[(i + offset) % peers.len()]).unwrap();
            let scans = REPLAY_SCAN_STEPS.with(|c| c.get());
            assert!(
                scans < 20 * N * peers.len(),
                "peers={peers:x?} prefix={shared_prefix} receiver={i} scans={scans}"
            );
        }
        assert_eq!(doc.get_text("text").to_string(), expected);
    }
}

#[test]
fn concurrent_text_import_has_linear_scan_work() {
    for peers in [
        vec![1, 2],
        vec![0x7000_0000_0000_0000, 0x7000_0000_0000_0001],
        vec![0xa5a5_a5a5_0000_0001, 0xa5a5_a5a5_0000_0002],
        vec![
            0x7000_0000_0000_0000,
            0xa5a5_a5a5_0000_0001,
            0xa5a5_a5a5_0000_0002,
        ],
    ] {
        check(&peers, false);
        // With three peers, the independent third branch forces replay past
        // the shared seed, releasing the other two branches mid-iteration.
        check(&peers, true);
    }
}
