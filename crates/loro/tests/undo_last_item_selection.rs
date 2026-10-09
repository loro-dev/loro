use loro::{cursor::Side, ExportMode, LoroDoc, UndoItemMeta, UndoManager};
use std::sync::{Arc, Mutex};

#[test]
fn last_undo_item_restores_selection_after_remote_edits() {
    // The control differs only by an older, unrelated undo item. Neither arm
    // imports a snapshot or clears the manager before editing.
    for older_item in [true, false] {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "Hello world!").unwrap();
        doc.commit();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        assert_eq!(undo.undo_count(), 0);

        if older_item {
            doc.get_map("unrelated").insert("key", true).unwrap();
            doc.commit();
        }
        let remaining = usize::from(older_item);
        assert_eq!(undo.undo_count(), remaining);

        let selection = [
            text.get_cursor(1, Side::Left).unwrap(),
            text.get_cursor(5, Side::Right).unwrap(),
        ];
        undo.set_on_push(Some(Box::new(move |_, _, _| {
            let mut meta = UndoItemMeta::new();
            for cursor in &selection {
                meta.add_cursor(cursor);
            }
            meta
        })));
        let popped = Arc::new(Mutex::new(Vec::new()));
        let captured = popped.clone();
        undo.set_on_pop(Some(Box::new(move |_, _, meta| {
            *captured.lock().unwrap() = meta.cursors;
        })));

        text.delete(1, 4).unwrap();
        doc.commit();
        assert_eq!(undo.undo_count(), remaining + 1);
        let peer = doc.fork();
        peer.set_peer_id(2).unwrap();
        peer.get_text("text").insert(0, "Hi ").unwrap();
        peer.get_text("text").insert(4, "ii").unwrap();
        peer.commit();
        doc.import(&peer.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
            .unwrap();
        assert_eq!(undo.undo_count(), remaining + 1);
        assert!(undo.undo().unwrap());
        assert_eq!(undo.undo_count(), remaining);
        assert_eq!(text.to_string(), "Hi Helloii world!");

        let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
        assert_eq!(restored.get_deep_value(), doc.get_deep_value());
        assert_eq!(restored.oplog_vv(), doc.oplog_vv());
        peer.import(&doc.export(ExportMode::updates(&peer.oplog_vv())).unwrap())
            .unwrap();
        assert_eq!(peer.get_deep_value(), doc.get_deep_value());
        assert_eq!(peer.oplog_vv(), doc.oplog_vv());

        let cursors = popped.lock().unwrap();
        let positions: Vec<_> = cursors
            .iter()
            .map(|cursor| doc.get_cursor_pos(&cursor.cursor).unwrap().current.pos)
            .collect();
        assert_eq!(positions, vec![4, 8], "older_item={older_item}");
        assert_eq!(
            cursors
                .iter()
                .map(|cursor| cursor.pos.pos)
                .collect::<Vec<_>>(),
            vec![4, 8],
            "older_item={older_item}"
        );
    }
}
