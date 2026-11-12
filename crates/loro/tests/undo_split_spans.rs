//! Regressions for #1245 and #1228: an inverse must not travel through
//! the next local span and back through its inverse before composition.
use loro::{ExportMode, LoroDoc, ToJson, UndoManager};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::json;

fn send(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
        .unwrap();
}

fn pair() -> (LoroDoc, LoroDoc) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    (a, b)
}

#[test]
fn undo_split_backspaces_preserves_order_and_remote_text() {
    for count in [2, 3, 5] {
        for remote_text in [false, true] {
            let (a, b) = pair();
            let text = a.get_text("text");
            text.insert(0, "abcdef").unwrap();
            a.commit();
            let mut undo = UndoManager::new(&a);
            undo.group_start().unwrap();
            for i in 0..count {
                text.delete(text.len_unicode() - 1, 1).unwrap();
                a.commit();
                send(&a, &b);
                b.get_map("meta").insert("step", i as i32).unwrap();
                if remote_text && i + 1 == count {
                    b.get_text("text").insert(0, "Z").unwrap();
                }
                b.commit();
                send(&b, &a);
            }
            undo.group_end();
            let after = text.to_string();
            let prefix = if remote_text {
                "Z".to_owned()
            } else {
                String::new()
            };
            for _ in 0..3 {
                assert!(undo.undo().unwrap());
                assert_eq!(text.to_string(), format!("{prefix}abcdef"));
                assert!(!undo.can_undo());
                assert_eq!(
                    a.get_map("meta").get("step").unwrap().into_value().unwrap(),
                    (count as i32 - 1).into()
                );
                assert!(undo.redo().unwrap());
                assert_eq!(text.to_string(), after);
            }
        }
    }
}

#[test]
fn undo_split_group_does_not_revive_intermediate_text() {
    for import_before_end in [false, true] {
        for separate_commits in [false, true] {
            let (a, b) = pair();
            let text = a.get_text("text");
            let mut undo = UndoManager::new(&a);
            undo.set_merge_interval(0);
            undo.group_start().unwrap();
            text.insert(0, "x").unwrap();
            a.commit();
            send(&a, &b);
            b.get_map("metadata").insert("flag", true).unwrap();
            b.commit();
            if import_before_end {
                send(&b, &a);
            }
            text.delete(0, 1).unwrap();
            if separate_commits {
                a.commit();
            }
            text.insert(0, "y").unwrap();
            a.commit();
            undo.group_end();
            if !import_before_end {
                send(&b, &a);
            }
            for _ in 0..3 {
                assert!(undo.undo().unwrap());
                assert_eq!(text.to_string(), "");
                assert!(!undo.can_undo());
                assert_eq!(
                    a.get_map("metadata").get_value().to_json_value(),
                    json!({"flag": true})
                );
                assert!(undo.redo().unwrap());
                assert_eq!(text.to_string(), "y");
            }
        }
    }
}

#[test]
fn undo_split_random_groups_restore_before_and_redo_after() {
    // Fixed seeds make failures reproducible. Remote text edits use a separate
    // prefix anchored before an untouched character, so restoration is unambiguous.
    for seed in 0..2000 {
        let mut rng = StdRng::seed_from_u64(seed);
        let (a, b) = pair();
        let text = a.get_text("text");
        let before = "#abcdefghijklmnop";
        text.insert(0, before).unwrap();
        a.commit();
        let mut undo = UndoManager::new(&a);
        undo.group_start().unwrap();
        let prefix = rng.gen_range(0..4);
        let rounds = rng.gen_range(2..10);
        for round in 0..rounds {
            let len = text.len_unicode() - 1;
            if len > 0 && rng.gen_bool(0.6) {
                let pos = rng.gen_range(0..len);
                let n = rng.gen_range(1..=len - pos);
                text.delete(1 + pos, n).unwrap();
            } else {
                let pos = rng.gen_range(0..=len);
                text.insert(1 + pos, &format!("{round}xyz")).unwrap();
            }
            a.commit();
            send(&a, &b);
            b.get_map("meta").insert("round", round).unwrap();
            b.commit();
            send(&b, &a);
        }
        undo.group_end();
        // An import touching a grouped container closes the manual group.
        // Import remote text only after all local spans have been recorded.
        if prefix > 0 {
            b.get_text("text").insert(0, &"Z".repeat(prefix)).unwrap();
            b.commit();
            send(&b, &a);
        }
        let after = text.to_string();
        let remote = a.get_map("meta").get_value();
        assert!(undo.undo().unwrap(), "seed {seed}");
        assert_eq!(
            text.to_string(),
            format!("{}{before}", "Z".repeat(prefix)),
            "seed {seed}"
        );
        assert_eq!(a.get_map("meta").get_value(), remote, "seed {seed}");
        assert!(undo.redo().unwrap(), "seed {seed}");
        assert_eq!(text.to_string(), after, "seed {seed}");
        assert_eq!(a.get_map("meta").get_value(), remote, "seed {seed}");
        send(&a, &b);
        assert_eq!(a.get_deep_value(), b.get_deep_value(), "seed {seed}");
    }
}

#[test]
fn undo_split_mixed_containers_retains_remote_edits() {
    let (a, b) = pair();
    let text = a.get_text("text");
    text.insert(0, "abcdef").unwrap();
    let list = a.get_list("list");
    let movable = a.get_movable_list("movable");
    for i in 0..6 {
        list.push(i).unwrap();
        movable.push(i).unwrap();
    }
    a.get_map("map").insert("local", 0).unwrap();
    a.commit();
    let mut undo = UndoManager::new(&a);
    undo.group_start().unwrap();
    for i in 0..3 {
        if i == 2 {
            // These edits are concurrent with the final local span. Delay
            // importing them until group_end so they do not close the group.
            b.get_text("text").mark(1..2, "italic", true).unwrap();
            b.get_list("list").insert(0, -1).unwrap();
            b.get_movable_list("movable").insert(0, -1).unwrap();
            b.get_map("map").insert("remote", 2).unwrap();
            b.commit();
        }
        text.delete(text.len_unicode() - 1, 1).unwrap();
        text.mark(0..1, "bold", true).unwrap();
        list.delete(list.len() - 1, 1).unwrap();
        movable.delete(movable.len() - 1, 1).unwrap();
        a.get_map("map").insert("local", i + 1).unwrap();
        a.commit();
        send(&a, &b);
        b.get_map("meta").insert("step", i).unwrap();
        b.commit();
        if i < 2 {
            send(&b, &a);
        }
    }
    undo.group_end();
    send(&b, &a);
    let after = a.get_deep_value();
    for _ in 0..3 {
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "abcdef");
        assert_eq!(
            text.get_richtext_value().to_json_value(),
            json!([
                {"insert":"a"}, {"insert":"b", "attributes":{"italic":true}}, {"insert":"cdef"}
            ])
        );
        assert_eq!(
            list.get_value().to_json_value(),
            json!([-1, 0, 1, 2, 3, 4, 5])
        );
        assert_eq!(
            movable.get_value().to_json_value(),
            json!([-1, 0, 1, 2, 3, 4, 5])
        );
        assert_eq!(
            a.get_map("map").get_value().to_json_value(),
            json!({"local":0,"remote":2})
        );
        assert!(undo.redo().unwrap());
        assert_eq!(a.get_deep_value(), after);
    }
}

#[test]
fn undo_split_group_revives_movable_child_with_earlier_group_edits_removed() {
    let (a, b) = pair();
    let parent = a.get_list("parent");
    let child = parent.push_container(loro::LoroMovableList::new()).unwrap();
    child.push(7).unwrap();
    a.commit();
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    child.push(8).unwrap();
    a.commit();
    undo.group_start().unwrap();
    child.push(9).unwrap();
    a.commit();
    send(&a, &b);
    b.get_map("meta").insert("flag", true).unwrap();
    b.commit();
    send(&b, &a);
    parent.delete(0, 1).unwrap();
    a.commit();
    undo.group_end();
    for _ in 0..3 {
        assert!(undo.undo().unwrap());
        assert_eq!(
            a.get_deep_value().to_json_value(),
            json!({"parent":[[7,8]], "meta":{"flag":true}})
        );
        assert!(undo.redo().unwrap());
        assert_eq!(parent.len(), 0);
    }
}
