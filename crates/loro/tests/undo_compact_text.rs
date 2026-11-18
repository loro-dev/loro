use loro::{ExpandType, ExportMode, LoroDoc, StyleConfig, StyleConfigMap, UndoManager};
use rand::{rngs::StdRng, Rng, SeedableRng};

#[test]
fn compact_insertions_restore_styled_unicode_across_batches_and_groups() {
    for seed in 0..96 {
        let doc = LoroDoc::new();
        doc.set_peer_id(seed + 1).unwrap();
        let mut styles = StyleConfigMap::new();
        styles.insert(
            "style".into(),
            StyleConfig::new().expand(match seed % 4 {
                0 => ExpandType::None,
                1 => ExpandType::Before,
                2 => ExpandType::After,
                _ => ExpandType::Both,
            }),
        );
        doc.config_text_style(styles);
        let text = doc.get_text("text");
        text.insert(0, "a汉😀e\u{301}z").unwrap();
        text.mark(1..4, "style", true).unwrap();
        doc.commit();
        let before = text.to_delta();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let mut random = StdRng::seed_from_u64(seed);
        for round in 0..4 {
            undo.group_start().unwrap();
            let mut run_start = random.gen_range(0..=text.len_unicode());
            let mut run_length = 0;
            for step in 0..48 {
                let content = ["a", "汉", "😀", "e\u{301}", "👩\u{200d}🚀"][random.gen_range(0..5)];
                let position = run_start + random.gen_range(0..=run_length);
                text.insert(position, content).unwrap();
                run_length += content.chars().count();
                if round == 2 && step % 7 == 6 {
                    let position = random.gen_range(0..text.len_unicode());
                    text.delete(position, 1).unwrap();
                    run_start = random.gen_range(0..=text.len_unicode());
                    run_length = 0;
                }
                if round == 3 && step % 11 == 10 {
                    let position = random.gen_range(0..text.len_unicode());
                    text.mark(position..position + 1, "style", false).unwrap();
                }
                if round % 2 == 1 {
                    doc.commit();
                }
            }
            doc.commit();
            undo.group_end();
            let after = text.to_delta();
            assert!(undo.undo().unwrap(), "seed={seed} round={round}");
            assert_eq!(text.to_delta(), before, "seed={seed} round={round}");
            assert!(undo.redo().unwrap());
            assert_eq!(text.to_delta(), after, "seed={seed} round={round}");
            assert!(undo.undo().unwrap());
        }
    }
}

#[test]
fn compact_insertions_keep_concurrent_remote_text_when_undone() {
    for seed in 0..64 {
        let local = LoroDoc::new();
        local.set_peer_id(1).unwrap();
        let text = local.get_text("text");
        text.insert(0, "base").unwrap();
        local.commit();
        let remote = local.fork();
        remote.set_peer_id(2).unwrap();
        let mut undo = UndoManager::new(&local);
        undo.set_merge_interval(0);
        let mut random = StdRng::seed_from_u64(seed);
        for _ in 0..32 {
            text.insert(random.gen_range(0..=text.len_unicode()), "汉😀")
                .unwrap();
        }
        local.commit();
        let remote_text = remote.get_text("text");
        remote_text.insert(2, "REMOTE").unwrap();
        remote.commit();
        local
            .import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let merged = text.to_delta();
        assert!(undo.undo().unwrap(), "seed={seed}");
        assert_eq!(text.to_delta(), remote_text.to_delta(), "seed={seed}");
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_delta(), merged, "seed={seed}");
        remote
            .import(&local.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(remote_text.to_delta(), text.to_delta(), "seed={seed}");
    }
}

#[test]
fn grouped_compact_insertions_preserve_remote_edits() {
    for remote_position in 0..=4 {
        let local = LoroDoc::new();
        local.set_peer_id(1).unwrap();
        let text = local.get_text("text");
        text.insert(0, "base").unwrap();
        local.commit();
        let remote = local.fork();
        remote.set_peer_id(2).unwrap();
        let mut undo = UndoManager::new(&local);
        undo.group_start().unwrap();
        for offset in 0..128 {
            text.insert(2 + offset, "汉").unwrap();
            local.commit();
        }
        undo.group_end();
        let remote_text = remote.get_text("text");
        remote_text.insert(remote_position, "😀").unwrap();
        remote.commit();
        local
            .import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let merged = text.to_delta();
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_delta(), remote_text.to_delta());
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_delta(), merged);
    }
}

#[test]
fn inverse_registration_survives_replacing_an_undo_manager() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "before").unwrap();
    doc.commit();
    let mut first = UndoManager::new(&doc);
    first.set_merge_interval(0);
    text.insert(6, "first").unwrap();
    doc.commit();
    assert!(first.undo().unwrap());
    drop(first);
    text.insert(6, "between").unwrap();
    doc.commit();
    let mut second = UndoManager::new(&doc);
    text.insert(13, "second").unwrap();
    doc.commit();
    assert!(second.undo().unwrap());
    assert_eq!(text.to_string(), "beforebetween");
    assert!(second.redo().unwrap());
    assert_eq!(text.to_string(), "beforebetweensecond");
}

#[test]
fn grouped_general_inverses_preserve_remote_edits_across_undo_redo() {
    for scenario in ["map_tail", "two_texts", "list", "list_mixed"] {
        for remote_position in 0..=4 {
            let local = LoroDoc::new();
            local.set_peer_id(1).unwrap();
            let text = local.get_text("text");
            let other = local.get_text("other");
            let list = local.get_list("list");
            let map = local.get_map("map");
            text.insert(0, "base").unwrap();
            other.insert(0, "base").unwrap();
            for value in 0..4 {
                list.push(value).unwrap();
            }
            map.insert("value", 0).unwrap();
            local.commit();
            let remote = local.fork();
            remote.set_peer_id(2).unwrap();
            let mut undo = UndoManager::new(&local);
            undo.group_start().unwrap();
            for offset in 0..128 {
                match scenario {
                    "map_tail" => text.insert(2 + offset, "汉").unwrap(),
                    "two_texts" => {
                        text.insert(2 + offset, "汉").unwrap();
                        other.insert(2 + offset, "😀").unwrap();
                    }
                    "list" => list.insert(2 + offset, offset as i32).unwrap(),
                    "list_mixed" => {
                        list.insert(2, offset as i32).unwrap();
                        if offset % 2 == 1 {
                            list.delete(2, 1).unwrap();
                        }
                    }
                    _ => unreachable!(),
                }
                local.commit();
            }
            if scenario == "map_tail" {
                map.insert("value", 1).unwrap();
                local.commit();
            }
            undo.group_end();
            remote
                .get_text("text")
                .insert(remote_position, "R")
                .unwrap();
            remote
                .get_text("other")
                .insert(remote_position, "S")
                .unwrap();
            remote.get_list("list").insert(remote_position, -1).unwrap();
            remote.commit();
            local
                .import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            let merged = local.get_deep_value();
            for _ in 0..2 {
                assert!(undo.undo().unwrap(), "{scenario} at {remote_position}");
                assert_eq!(local.get_deep_value(), remote.get_deep_value());
                assert!(undo.redo().unwrap());
                assert_eq!(local.get_deep_value(), merged);
            }
        }
    }
}

#[test]
fn grouped_list_insertions_coalesce_compact_inverses_in_place() {
    let doc = LoroDoc::new();
    let list = doc.get_list("list");
    list.push("prefix").unwrap();
    list.push("suffix").unwrap();
    doc.commit();
    let before = doc.get_deep_value();

    let mut undo = UndoManager::new(&doc);
    undo.group_start().unwrap();
    for i in 0..2000 {
        list.insert(1 + i, i as i64).unwrap();
        doc.commit();
    }
    undo.group_end();

    let stats = doc.memory_stats();
    assert!(
        stats.undo_bytes < 4096,
        "2000 grouped contiguous list inserts should coalesce into a single compact inverse, got {} bytes",
        stats.undo_bytes
    );

    let after = doc.get_deep_value();
    assert!(undo.undo().unwrap());
    assert_eq!(doc.get_deep_value(), before);
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_deep_value(), after);
}
