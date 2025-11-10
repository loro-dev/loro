#![allow(deprecated)]
#![allow(unexpected_cfgs)]
use std::sync::Arc;

use loro::{ToJson as _, ID};

///
/// This macro creates a series of random operations on various container types
/// within a LoroDoc. It's useful for testing and fuzzing purposes.
///
/// # Parameters
///
/// * `$doc` - A reference to a `loro::LoroDoc` instance.
/// * `$seed` - A `u64` value used to seed the random number generator.
/// * `$len` - A `usize` value specifying the number of random operations to perform.
///
/// # Example
///
/// ```
/// let doc = loro::LoroDoc::new();
/// gen_random_ops!(doc, 12345, 100);
/// ```
#[macro_export]
macro_rules! gen_random_ops {
    ($doc:expr, $seed:expr, $len:expr) => {{
        use rand::rngs::StdRng;
        use rand::seq::SliceRandom;
        use rand::{Rng, SeedableRng};

        let mut rng = StdRng::seed_from_u64($seed);
        let containers = ["text", "map", "list", "tree", "movable_list"];

        for _ in 0..$len {
            let container = containers.choose(&mut rng).unwrap();
            match *container {
                "text" => {
                    let text = $doc.get_text("text");
                    let pos = rng.gen_range(0..=text.len_unicode());
                    let content = (0..5).map(|_| rng.gen::<char>()).collect::<String>();
                    if rng.gen_bool(0.7) {
                        text.insert(pos, &content).unwrap_or_default();
                    } else {
                        let del_len = rng.gen_range(0..=text.len_unicode().saturating_sub(pos));
                        text.delete(pos, del_len).unwrap_or_default();
                    }
                }
                "map" => {
                    let map = $doc.get_map("map");
                    let key = format!("key_{}", rng.gen::<u32>());
                    if rng.gen_bool(0.7) {
                        let value = format!("value_{}", rng.gen::<u32>());
                        map.insert(&key, value).unwrap();
                    } else if !map.is_empty() {
                        let v = map.get_value();
                        let existing_key = v
                            .as_map()
                            .unwrap()
                            .keys()
                            .nth(rng.gen_range(0..map.len()))
                            .unwrap();
                        map.delete(&existing_key).unwrap();
                    }
                }
                "list" => {
                    let list = $doc.get_list("list");
                    let pos = rng.gen_range(0..=list.len());
                    if rng.gen_bool(0.7) {
                        let value = rng.gen::<i32>();
                        list.insert(pos, value).unwrap();
                    } else if !list.is_empty() {
                        list.delete(pos, 1).unwrap_or_default();
                    }
                }
                "tree" => {
                    let tree = $doc.get_tree("tree");
                    tree.enable_fractional_index(0);
                    let nodes: Vec<_> = tree.nodes();
                    if nodes.is_empty() {
                        tree.create(None).unwrap();
                    } else {
                        let node = nodes.choose(&mut rng).unwrap();
                        match rng.gen_range(0..3) {
                            0 => {
                                tree.create(Some(*node)).unwrap();
                            }
                            1 if !tree.is_node_deleted(node).unwrap() => {
                                tree.delete(*node).unwrap_or_default();
                            }
                            _ => {
                                if let Some(sibling) = nodes.choose(&mut rng) {
                                    if sibling != node
                                        && !tree.is_node_deleted(sibling).unwrap()
                                        && !tree.is_node_deleted(node).unwrap()
                                    {
                                        tree.mov_before(*node, *sibling).unwrap_or_default();
                                    }
                                }
                            }
                        }
                    }
                }
                "movable_list" => {
                    let movable_list = $doc.get_movable_list("movable_list");
                    let pos = rng.gen_range(0..=movable_list.len());
                    match rng.gen_range(0..3) {
                        0 => {
                            let value = rng.gen::<i32>();
                            movable_list.insert(pos, value).unwrap();
                        }
                        1 => {
                            if !movable_list.is_empty() {
                                movable_list.delete(pos, 1).unwrap_or_default();
                            }
                        }
                        2 => {
                            if !movable_list.is_empty() {
                                let from = rng.gen_range(0..movable_list.len());
                                let to = rng.gen_range(0..=movable_list.len());
                                movable_list.mov(from, to).unwrap_or_default();
                            }
                        }
                        _ => unreachable!("unreachable movable list op"),
                    }
                }
                _ => unreachable!("unreachable container type"),
            }
        }
        $doc.commit();
    }};
}
#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

mod compatibility_with_10_alpha_4 {
    use super::*;
    use loro_alpha_4::{self, ToJson};

    #[test]
    fn test_shallow_snapshot() {
        let doc1 = loro::LoroDoc::new();
        doc1.set_peer_id(1).unwrap();
        gen_random_ops!(doc1, 12345, 100);
        let snapshot = doc1
            .export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 50)))
            .unwrap();

        let doc2 = loro_alpha_4::LoroDoc::new();
        doc2.import(&snapshot).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );
        assert_eq!(
            doc2.shallow_since_frontiers().encode(),
            loro::Frontiers::from_id(ID::new(1, 50)).encode()
        );

        gen_random_ops!(doc2, 12345, 100);
        let snapshot = doc2
            .export(loro_alpha_4::ExportMode::all_updates())
            .unwrap();
        doc1.import(&snapshot).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );
    }

    #[test]
    fn test_shallow_snapshot_mirrored() {
        let doc1 = loro_alpha_4::LoroDoc::new();
        doc1.set_peer_id(1).unwrap();
        gen_random_ops!(doc1, 1234, 100);
        let snapshot = doc1
            .export(loro_alpha_4::ExportMode::shallow_snapshot_since(
                loro_alpha_4::ID::new(1, 50),
            ))
            .unwrap();

        let doc2 = loro::LoroDoc::new();
        doc2.import(&snapshot).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );
        assert_eq!(
            doc2.shallow_since_frontiers().encode(),
            loro_alpha_4::Frontiers::from_id(loro_alpha_4::ID::new(1, 50)).encode()
        );

        gen_random_ops!(doc2, 1234, 100);
        let snapshot = doc2.export(loro::ExportMode::all_updates()).unwrap();
        doc1.import(&snapshot).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );
    }

    #[test]
    fn test_snapshot() {
        let doc1 = loro_alpha_4::LoroDoc::new();
        doc1.set_peer_id(1).unwrap();
        gen_random_ops!(doc1, 1234, 100);
        let snapshot = doc1.export(loro_alpha_4::ExportMode::Snapshot).unwrap();

        let doc2 = loro::LoroDoc::new();
        doc2.import(&snapshot).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );

        gen_random_ops!(doc2, 5678, 100);
        let snapshot = doc2.export(loro::ExportMode::Snapshot).unwrap();
        doc1.import(&snapshot).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );

        let updates =
            serde_json::to_value(doc1.export_json_updates(&Default::default(), &doc1.oplog_vv()))
                .unwrap();
        let updates_b =
            serde_json::to_value(doc2.export_json_updates(&Default::default(), &doc2.oplog_vv()))
                .unwrap();
        assert_eq!(updates, updates_b);
    }

    #[test]
    fn test_updates() {
        let doc1 = loro_alpha_4::LoroDoc::new();
        let doc2 = Arc::new(loro::LoroDoc::new());
        let doc2_clone = doc2.clone();
        doc1.set_peer_id(1).unwrap();
        doc1.subscribe_local_update(Box::new(move |updates| {
            doc2_clone.import(updates).unwrap();
            true
        }))
        .detach();

        for i in 0..5 {
            gen_random_ops!(doc1, i, 10);
            assert_eq!(
                doc1.get_deep_value().to_json_value(),
                doc2.get_deep_value().to_json_value()
            );
        }
    }

    #[test]
    fn test_update_in_range() {
        let doc1 = loro_alpha_4::LoroDoc::new();
        let doc2 = loro::LoroDoc::new();

        // Generate some initial content
        gen_random_ops!(doc1, 1234, 50);
        let version = doc1.oplog_vv();
        let doc1_value = doc1.get_deep_value().to_json_value();
        gen_random_ops!(doc1, 1234, 50);
        let updates = doc1
            .export(loro_alpha_4::ExportMode::updates_till(&version))
            .unwrap();

        doc2.import(&updates).unwrap();
        assert_eq!(doc1_value, doc2.get_deep_value().to_json_value());
    }

    #[test]
    fn test_json_updates() {
        let doc1 = loro_alpha_4::LoroDoc::new();
        let doc2 = loro::LoroDoc::new();

        gen_random_ops!(doc1, 0, 1000);
        let updates =
            serde_json::to_string(&doc1.export_json_updates(&Default::default(), &doc1.oplog_vv()))
                .unwrap();
        doc2.import_json_updates(updates).unwrap();
        assert_eq!(
            doc1.get_deep_value().to_json_value(),
            doc2.get_deep_value().to_json_value()
        );
    }
}
