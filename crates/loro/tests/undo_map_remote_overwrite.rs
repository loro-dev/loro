//! A local undo/redo must not clobber another peer's write that wins a map key
//! (the map counterpart of `undo_text_style_concurrent.rs`). See
//! `context/undo-log.md`, "Map keys another peer overwrote".

use loro::{ExportMode, LoroDoc, LoroMap, LoroValue, ToJson, UndoManager};
use serde_json::json;

fn peer(id: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(id).unwrap();
    doc
}

fn sync(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
        .unwrap();
}

fn undo_manager(doc: &LoroDoc) -> UndoManager {
    let mut undo = UndoManager::new(doc);
    undo.set_merge_interval(0);
    undo
}

#[test]
fn undo_keeps_a_later_remote_overwrite() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    a.get_map("m").insert("k", "a").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);

    assert!(undo.can_undo());
    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
    sync(&a, &b);
    assert_eq!(a.get_deep_value(), b.get_deep_value());
}

#[test]
fn undo_keeps_a_concurrent_remote_write_that_wins() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    a.get_map("m").insert("k", "a").unwrap();
    a.commit();
    // Same lamport, higher peer: B's write wins.
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

#[test]
fn undo_keeps_a_remote_delete() {
    let a = peer(1);
    let b = peer(2);
    let map = a.get_map("m");
    map.insert("k", "old").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = undo_manager(&a);
    map.insert("k", "a").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").delete("k").unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {}}));
}

#[test]
fn undo_still_reverts_keys_the_remote_did_not_write() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let map = a.get_map("m");
    map.insert("k", "a").unwrap();
    map.insert("j", "a").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
    // Redo writes `j` back; `k` stays B's.
    undo.redo().unwrap();
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"m": {"k": "b", "j": "a"}})
    );
}

#[test]
fn redo_keeps_a_remote_write_made_after_the_undo() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    a.get_map("m").insert("k", "a").unwrap();
    a.commit();
    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {}}));
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);

    assert!(undo.can_redo());
    undo.redo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

#[test]
fn undo_keeps_a_remote_overwrite_of_a_child_container() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let child = a
        .get_map("m")
        .insert_container("k", LoroMap::new())
        .unwrap();
    child.insert("x", 1).unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

#[test]
fn undo_keeps_a_remote_child_container_written_over_the_key() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    a.get_map("m").insert("k", "a").unwrap();
    a.commit();
    sync(&a, &b);
    let child = b
        .get_map("m")
        .insert_container("k", LoroMap::new())
        .unwrap();
    child.insert("x", 1).unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"m": {"k": {"x": 1}}})
    );
}

#[test]
fn undo_of_a_grouped_item_keeps_a_remote_write_between_its_commits() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let map = a.get_map("m");
    undo.group_start().unwrap();
    map.insert("k", "a1").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);
    map.insert("j", "a2").unwrap();
    a.commit();
    undo.group_end();

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

#[test]
fn undo_of_a_grouped_item_reverts_its_own_write_over_a_remote_one() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let map = a.get_map("m");
    undo.group_start().unwrap();
    map.insert("k", "a1").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);
    // A overwrites B inside the same item: undoing the item brings B's value back.
    map.insert("k", "a2").unwrap();
    a.commit();
    undo.group_end();

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

#[test]
fn undo_still_reverts_a_local_write_over_a_remote_one() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let map = a.get_map("m");
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);
    map.insert("k", "a").unwrap();
    a.commit();

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
    undo.redo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "a"}}));
}

#[test]
fn undoing_two_items_keeps_a_remote_write_between_them() {
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let map = a.get_map("m");
    map.insert("k", "a1").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);
    map.insert("k", "a2").unwrap();
    a.commit();

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

#[test]
fn undo_keeps_a_remote_tree_meta_overwrite() {
    let a = peer(1);
    let b = peer(2);
    let tree = a.get_tree("t");
    let node = tree.create(None).unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    tree.get_meta(node).unwrap().insert("k", "a").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_tree("t")
        .get_meta(node)
        .unwrap()
        .insert("k", "b")
        .unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(
        a.get_tree("t").get_meta(node).unwrap().get_value(),
        LoroValue::from(json!({"k": "b"}))
    );
}

#[test]
fn undo_keeps_a_remote_movable_list_set() {
    let a = peer(1);
    let b = peer(2);
    let list = a.get_movable_list("l");
    list.push("old").unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    list.set(0, "a").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_movable_list("l").set(0, "b").unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"l": ["b"]}));
}

#[test]
fn redo_keeps_a_remote_movable_list_set() {
    let a = peer(1);
    let b = peer(2);
    let list = a.get_movable_list("l");
    list.push("old").unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    list.set(0, "a").unwrap();
    a.commit();
    undo.undo().unwrap();
    sync(&a, &b);
    b.get_movable_list("l").set(0, "b").unwrap();
    b.commit();
    sync(&b, &a);

    undo.redo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"l": ["b"]}));
}

#[test]
fn undo_still_reverts_a_movable_list_set_a_remote_only_moved() {
    let a = peer(1);
    let b = peer(2);
    let list = a.get_movable_list("l");
    list.push("old").unwrap();
    list.push("x").unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    list.set(0, "a").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_movable_list("l").mov(0, 1).unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"l": ["x", "old"]})
    );
}

#[test]
fn undo_still_reverts_a_movable_list_set_after_undo_recreated_the_element() {
    let a = peer(1);
    let list = a.get_movable_list("l");
    list.push("old").unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    list.set(0, "a").unwrap();
    a.commit();
    list.delete(0, 1).unwrap();
    a.commit();

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"l": ["a"]}));
    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"l": ["old"]}));
}

#[test]
fn undoing_two_local_items_keeps_a_remote_write_after_both() {
    // Both items share one stack row and its remote diff. Undoing the first
    // must not drop the remote's key from that diff (`transform_remote_over_step`).
    let a = peer(1);
    let b = peer(2);
    let mut undo = undo_manager(&a);
    let map = a.get_map("m");
    map.insert("k", "a1").unwrap();
    a.commit();
    map.insert("k", "a2").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_map("m").insert("k", "b").unwrap();
    b.commit();
    sync(&b, &a);

    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
    undo.undo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
    undo.redo().unwrap();
    undo.redo().unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"m": {"k": "b"}}));
}

// An undo that restores another peer's value writes it with a new op of this
// peer. Later undos must still treat it as the other peer's write.

fn list_values(doc: &LoroDoc) -> serde_json::Value {
    doc.get_deep_value().to_json_value()["l"].clone()
}

#[test]
fn undo_twice_keeps_a_remote_movable_list_set_an_undo_restored() {
    let a = peer(1);
    let b = peer(2);
    let list = a.get_movable_list("l");
    list.push("old").unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    list.set(0, "a1").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_movable_list("l").set(0, "b").unwrap();
    b.commit();
    sync(&b, &a);
    list.set(0, "a2").unwrap();
    a.commit();

    undo.undo().unwrap();
    assert_eq!(list_values(&a), json!(["b"]));
    // A no-op: the value is B's, even though the first undo wrote it.
    undo.undo().unwrap();
    assert_eq!(list_values(&a), json!(["b"]));
    // The no-op pushed no redo item (context/undo-history-boundaries.md).
    undo.redo().unwrap();
    assert_eq!(list_values(&a), json!(["a2"]));
    assert!(!undo.can_redo());
    undo.undo().unwrap();
    assert_eq!(list_values(&a), json!(["b"]));
    assert!(!undo.can_undo());
}

#[test]
fn undo_twice_keeps_a_remote_movable_list_value_an_undo_recreated() {
    let a = peer(1);
    let b = peer(2);
    let list = a.get_movable_list("l");
    list.push("old").unwrap();
    a.commit();
    let mut undo = undo_manager(&a);
    list.set(0, "a1").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_movable_list("l").set(0, "b").unwrap();
    b.commit();
    sync(&b, &a);
    list.delete(0, 1).unwrap();
    a.commit();

    undo.undo().unwrap();
    assert_eq!(list_values(&a), json!(["b"]));
    undo.undo().unwrap();
    assert_eq!(list_values(&a), json!(["b"]));
    undo.redo().unwrap();
    assert_eq!(list_values(&a), json!([]));
    assert!(!undo.can_redo());
    undo.undo().unwrap();
    assert_eq!(list_values(&a), json!(["b"]));
    assert!(!undo.can_undo());
}

#[test]
fn undo_twice_keeps_a_remote_map_value_an_undo_restored() {
    for delete_instead in [false, true] {
        let a = peer(1);
        let b = peer(2);
        let map = a.get_map("m");
        map.insert("k", "old").unwrap();
        a.commit();
        let mut undo = undo_manager(&a);
        map.insert("k", "a1").unwrap();
        a.commit();
        sync(&a, &b);
        b.get_map("m").insert("k", "b").unwrap();
        b.commit();
        sync(&b, &a);
        if delete_instead {
            map.delete("k").unwrap();
        } else {
            map.insert("k", "a2").unwrap();
        }
        a.commit();
        let last = if delete_instead {
            json!({"m": {}})
        } else {
            json!({"m": {"k": "a2"}})
        };
        let kept = json!({"m": {"k": "b"}});

        undo.undo().unwrap();
        assert_eq!(a.get_deep_value().to_json_value(), kept);
        undo.undo().unwrap();
        assert_eq!(a.get_deep_value().to_json_value(), kept);
        undo.redo().unwrap();
        assert_eq!(a.get_deep_value().to_json_value(), last);
        assert!(!undo.can_redo());
        undo.undo().unwrap();
        assert_eq!(a.get_deep_value().to_json_value(), kept);
        assert!(!undo.can_undo());
    }
}
