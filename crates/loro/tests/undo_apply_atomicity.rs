use loro::{ContainerTrait, ExportMode, LoroDoc, ToJson, UndoManager};
use serde_json::json;
fn send(a: &LoroDoc, b: &LoroDoc) {
    b.import_with(&a.export(ExportMode::all_updates()).unwrap(), "import")
        .unwrap();
}

// The original issue history must still succeed. Internal fault injection
// independently covers current main's application-error path.
#[test]
fn original_mergeable_revival_history_still_undoes_successfully() {
    for mixed in [false, true] {
        for map_first in [false, true] {
            let a = LoroDoc::new();
            a.set_peer_id(1).unwrap();
            if map_first {
                a.get_map("other").insert("base", 0).unwrap();
                a.commit();
            }
            let root = a.get_map("root");
            let la = root.ensure_mergeable_movable_list("child").unwrap();
            la.push(0).unwrap();
            a.commit();
            if !map_first {
                a.get_map("other").insert("base", 0).unwrap();
                a.commit();
            }
            let b = LoroDoc::new();
            b.set_peer_id(2).unwrap();
            send(&a, &b);
            let lb = b.get_movable_list(la.id());
            let mut ua = UndoManager::new(&a);
            ua.add_exclude_origin_prefix("import");
            let mut ub = UndoManager::new(&b);
            ub.add_exclude_origin_prefix("import");
            ub.set_merge_interval(0);
            b.get_map("other").insert("prior", 7).unwrap();
            b.commit();
            if mixed {
                b.get_map("other").insert("edited", 42).unwrap();
            }
            lb.push(1).unwrap();
            b.commit();
            root.delete("child").unwrap();
            a.commit();
            send(&a, &b);
            send(&b, &a);
            ua.undo().unwrap();
            send(&a, &b);
            // The historical mergeable-revival trigger is already repaired.
            assert_eq!(lb.get_value().to_json_value(), json!([0, 1]));
            assert!(ub.undo().unwrap());
            assert_eq!(lb.get_value().to_json_value(), json!([0]));
            assert_eq!(
                b.get_map("other").get_value().to_json_value()["prior"],
                json!(7)
            );
            assert!(b.get_map("other").get("edited").is_none());
        }
    }
}
