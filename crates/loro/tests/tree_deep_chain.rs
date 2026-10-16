use loro::{ExportMode, LoroDoc, LoroValue, TreeID, TreeParentId};

const LEVELS: usize = 4_000;

fn run_in_child(name: &str) -> bool {
    // Stack overflow aborts rather than unwinds. Keep a regression in a child
    // process so it reports a test failure instead of aborting the test runner.
    const CHILD: &str = "LORO_ISSUE_1195_CHILD";
    if std::env::var(CHILD).ok().as_deref() != Some(name) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CHILD, name)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child status: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return false;
    }
    true
}

#[test]
fn issue_1195_snapshot_first_read() {
    if !run_in_child("issue_1195_snapshot_first_read") {
        return;
    }

    // The issue's reproduction: build/export on a large stack, then import
    // and read the tree for the first time on a 1 MiB stack.
    let snapshot = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            let tree = doc.get_tree("t");
            let mut parent = TreeParentId::Root;
            for _ in 0..LEVELS {
                let id = tree.create(parent).unwrap();
                parent = TreeParentId::Node(id);
            }
            doc.commit();
            doc.export(ExportMode::Snapshot).unwrap()
        })
        .unwrap()
        .join()
        .unwrap();
    println!(
        "snapshot of a {LEVELS}-level chain: {} bytes",
        snapshot.len()
    );
    let n = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            println!("import ok");
            doc.get_tree("t").roots().len()
        })
        .unwrap()
        .join()
        .unwrap();
    println!("roots read: {n}");
    assert_eq!(n, 1);
}

fn chain_snapshot() -> (Vec<u8>, Vec<TreeID>) {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            let tree = doc.get_tree("t");
            let mut ids = Vec::new();
            let mut parent = TreeParentId::Root;
            for _ in 0..LEVELS {
                let id = tree.create(parent).unwrap();
                ids.push(id);
                parent = id.into();
            }
            doc.commit();
            (doc.export(ExportMode::Snapshot).unwrap(), ids)
        })
        .unwrap()
        .join()
        .unwrap()
}

fn assert_chain(value: &LoroValue, ids: &[TreeID], with_meta: bool) {
    let mut children = value.as_list().unwrap();
    for (depth, id) in ids.iter().enumerate() {
        assert_eq!(children.len(), 1, "depth {depth}");
        let node = children[0].as_map().unwrap();
        assert_eq!(node["id"].as_string().unwrap().as_str(), id.to_string());
        if depth == 0 {
            assert!(node["parent"].is_null());
        } else {
            assert_eq!(
                node["parent"].as_string().unwrap().as_str(),
                ids[depth - 1].to_string(),
            );
        }
        assert_eq!(node["index"].as_i64(), Some(&0));
        if with_meta {
            let meta = node["meta"].as_map().unwrap();
            if depth == ids.len() - 1 {
                assert_eq!(meta["leaf"].as_bool(), Some(&true));
            } else {
                assert!(meta.is_empty());
            }
        } else {
            assert!(node["meta"].is_container());
        }
        children = node["children"].as_list().unwrap();
    }
    assert!(children.is_empty());
}

#[test]
fn deep_tree_values_and_snapshot_on_small_stack() {
    if !run_in_child("deep_tree_values_and_snapshot_on_small_stack") {
        return;
    }
    let (snapshot, ids) = chain_snapshot();
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            // Each first-read entry point must independently decode the tree.
            for read in 0..4 {
                let doc = LoroDoc::new();
                doc.import(&snapshot).unwrap();
                let tree = doc.get_tree("t");
                match read {
                    0 => assert_eq!(tree.nodes().len(), LEVELS),
                    1 => assert_chain(&tree.get_value(), &ids, false),
                    2 => {
                        tree.get_meta(*ids.last().unwrap())
                            .unwrap()
                            .insert("leaf", true)
                            .unwrap();
                        assert_chain(&tree.get_value_with_meta(), &ids, true);
                    }
                    3 => {
                        tree.get_meta(*ids.last().unwrap())
                            .unwrap()
                            .insert("leaf", true)
                            .unwrap();
                        let value = doc.get_deep_value();
                        assert_chain(&value.as_map().unwrap()["t"], &ids, true);
                    }
                    _ => unreachable!(),
                }
                // Export's sibling/subtree walk must also work on this stack.
                let encoded = doc.export(ExportMode::Snapshot).unwrap();
                let copy = LoroDoc::new();
                copy.import(&encoded).unwrap();
                assert_eq!(copy.get_tree("t").get_nodes(false).len(), LEVELS);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn deep_tree_parent_walks_on_small_stack() {
    if !run_in_child("deep_tree_parent_walks_on_small_stack") {
        return;
    }
    let (snapshot, ids) = chain_snapshot();
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            let tree = doc.get_tree("t");
            let root = ids[0];
            let leaf = *ids.last().unwrap();
            assert!(!tree.is_node_deleted(&leaf).unwrap());
            assert!(tree.mov(root, leaf).is_err());
            assert_eq!(tree.parent(root), Some(TreeParentId::Root));
            assert_eq!(tree.nodes().len(), LEVELS);
            tree.delete(root).unwrap();
            assert!(tree.is_node_deleted(&leaf).unwrap());
            let encoded = doc.export(ExportMode::Snapshot).unwrap();
            let copy = LoroDoc::new();
            copy.import(&encoded).unwrap();
            assert!(copy.get_tree("t").is_node_deleted(&leaf).unwrap());
            assert_eq!(copy.get_tree("t").get_nodes(true).len(), LEVELS);
            tree.mov(leaf, TreeParentId::Root).unwrap();
            assert!(!tree.is_node_deleted(&leaf).unwrap());
            assert_eq!(tree.roots(), vec![leaf]);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn deep_tree_copy_and_shared_value_drop_on_small_stacks() {
    if !run_in_child("deep_tree_copy_and_shared_value_drop_on_small_stacks") {
        return;
    }
    let (snapshot, _) = chain_snapshot();
    let value = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            let tree = doc.get_tree("t");
            let copied = doc
                .get_map("copies")
                .insert_container("t", tree.clone())
                .unwrap();
            assert_eq!(copied.get_nodes(false).len(), LEVELS);
            tree.get_value()
        })
        .unwrap()
        .join()
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = [value.clone(), value]
        .into_iter()
        .map(|value| {
            let barrier = barrier.clone();
            std::thread::Builder::new()
                .stack_size(1 << 20)
                .spawn(move || {
                    barrier.wait();
                    drop(value);
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
}
