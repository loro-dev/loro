use loro::{LoroDoc, LoroValue};
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[test]
fn disallow_editing_on_detached_mode_by_default() {
    let doc = LoroDoc::new();
    let _sub = doc.subscribe_root(Arc::new(|b| {
        for e in b.events {
            if let loro::event::Diff::List(list_diff_items) = e.diff {
                let items = &list_diff_items[0];
                match items {
                    loro::event::ListDiffItem::Insert { insert, .. } => {
                        assert_eq!(insert[0].as_value().unwrap(), &LoroValue::I64(1));
                        assert_eq!(insert[1].as_value().unwrap(), &LoroValue::I64(2));
                    }
                    _ => {
                        unreachable!()
                    }
                }
            }
        }
    }));
    doc.get_list("l0").insert(0, 1).unwrap();
    doc.get_map("map").insert("key", "23").unwrap();
    // doc.get_movable_list("ml").insert(0, "23").unwrap();
    doc.get_list("l0").insert(1, 2).unwrap();
    doc.commit();
}
