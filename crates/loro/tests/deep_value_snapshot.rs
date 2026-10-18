use loro::{ExportMode, LoroDoc, LoroValue};

const MAP: &[u8] = include_bytes!("deep_value_map.bin");
const LISTS: &[u8] = include_bytes!("deep_value_lists.bin");

fn assert_chain(mut value: &LoroValue, mixed: bool) {
    for i in 0..20_000 {
        value = if mixed && i % 2 == 1 {
            value.as_list().unwrap().first().unwrap()
        } else {
            value.as_map().unwrap().get("a").unwrap()
        };
    }
    assert_eq!(value, &LoroValue::I64(0));
}

#[test]
fn issue_1198_first_read_and_drop_on_one_mib_stack() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            for bytes in [MAP, LISTS] {
                let doc = LoroDoc::new();
                doc.import(bytes).unwrap();
                assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
                if bytes == MAP {
                    let value = doc.get_map("root").get("v").unwrap().into_value().unwrap();
                    assert_chain(&value, false);
                    drop(value);
                } else {
                    let list = doc.get_list("list").get(0).unwrap().into_value().unwrap();
                    let movable = doc
                        .get_movable_list("movable")
                        .get(0)
                        .unwrap()
                        .into_value()
                        .unwrap();
                    assert_chain(&list, true);
                    assert_chain(&movable, true);
                }
                // Both an exported copy and the original must remain readable.
                let copy = LoroDoc::new();
                copy.import(&doc.export(ExportMode::Snapshot).unwrap())
                    .unwrap();
                assert_eq!(copy.state_frontiers(), doc.state_frontiers());
                drop(copy.get_deep_value());
                drop(copy);
                drop(doc.get_deep_value());
                drop(doc);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
