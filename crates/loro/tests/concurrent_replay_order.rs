use loro::{ExportMode, LoroDoc};

#[test]
fn pathological_peer_ids_converge_in_both_import_directions() {
    for low in [1, 0x7000_0000_0000_0000, 0xa5a5_a5a5_0000_0001] {
        let docs = [low, low + 1].map(|peer| {
            let doc = LoroDoc::new();
            doc.set_peer_id(peer).unwrap();
            for _ in 0..1_000 {
                doc.get_text("text")
                    .insert(0, if peer == low { "a" } else { "b" })
                    .unwrap();
                doc.commit();
            }
            doc
        });
        let updates = docs
            .each_ref()
            .map(|doc| doc.export(ExportMode::all_updates()).unwrap());
        for i in 0..2 {
            docs[i].import(&updates[1 - i]).unwrap();
        }
        let expected = "a".repeat(1_000) + &"b".repeat(1_000);
        for doc in docs {
            assert_eq!(doc.get_text("text").to_string(), expected);
        }
    }
}
