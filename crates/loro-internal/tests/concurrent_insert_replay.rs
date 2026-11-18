use loro_internal::{encoding::ExportMode, LoroDoc, VersionVector};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};

#[test]
fn out_of_order_insert_batches_match_cold_replay_and_checkout() {
    let mut rng = StdRng::seed_from_u64(73);
    let mut updates = Vec::new();
    let mut branches = Vec::new();
    for peer in 1..=192 {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(peer).unwrap();
        let text = doc.get_text("text");
        let list = doc.get_list("list");
        text.insert_unicode(0, "x😀").unwrap();
        list.insert(0, peer as i64).unwrap();
        doc.commit_then_renew();
        let base = doc.oplog_vv();
        branches.push(doc.oplog_frontiers());
        updates.push(doc.export(ExportMode::all_updates()).unwrap());
        text.insert_unicode(1, "中").unwrap();
        list.insert(1, -(peer as i64)).unwrap();
        updates.push(doc.export(ExportMode::updates(&base)).unwrap());
    }

    let batch = LoroDoc::new_auto_commit();
    updates.shuffle(&mut rng);
    for chunk in updates.chunks(13) {
        batch.import_batch(chunk).unwrap();
    }
    let replay = LoroDoc::new_auto_commit();
    replay
        .import(
            &batch
                .export(ExportMode::updates(&VersionVector::default()))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(replay.oplog_frontiers(), batch.oplog_frontiers());
    assert_eq!(replay.get_deep_value(), batch.get_deep_value());
    assert_eq!(batch.get_text("text").to_string(), "x中😀".repeat(192));
    let expected = batch.get_deep_value();
    for frontier in branches.choose_multiple(&mut rng, 20) {
        batch.checkout(frontier).unwrap();
        replay.checkout(frontier).unwrap();
        assert_eq!(batch.get_deep_value(), replay.get_deep_value());
        assert_eq!(batch.get_text("text").to_string(), "x😀");
        batch.attach();
        replay.attach();
        assert_eq!(batch.get_deep_value(), expected);
        assert_eq!(replay.get_deep_value(), expected);
    }
}
