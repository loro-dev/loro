use std::{
    collections::{BTreeMap, HashSet},
    convert::TryInto,
    ops::Bound::{Included, Unbounded},
};

use loro::{
    kv_store::mem_store::MemKvConfig, CommitOptions, ContainerTrait, EncodedBlobMode, ExportMode,
    Frontiers, IdSpan, JsonOpContent, JsonSchema, KvStore, LoroDoc, LoroList, LoroMap,
    LoroMovableList, LoroResult, LoroText, LoroTree, MemKvStore, ToJson, TreeParentId,
    VersionVector,
};
use pretty_assertions::assert_eq;
use serde_json::Value;

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn bytes_pairs<I, K, V>(iter: I) -> Vec<(Vec<u8>, Vec<u8>)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    iter.into_iter()
        .map(|(k, v)| (k.as_ref().to_vec(), v.as_ref().to_vec()))
        .collect()
}

fn collect_scan(
    store: &impl KvStore,
    start: std::ops::Bound<&[u8]>,
    end: std::ops::Bound<&[u8]>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    bytes_pairs(KvStore::scan(store, start, end))
}

fn collect_scan_rev(
    store: &impl KvStore,
    start: std::ops::Bound<&[u8]>,
    end: std::ops::Bound<&[u8]>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    bytes_pairs(KvStore::scan(store, start, end).rev())
}

fn nested_text(map: &LoroMap, key: &str) -> LoroText {
    let container = map
        .get(key)
        .unwrap_or_else(|| panic!("missing nested container {key}"))
        .into_container()
        .unwrap_or_else(|_| panic!("{key} is not a container"));
    LoroText::try_from_container(container).expect("nested text container")
}

fn nested_list(map: &LoroMap, key: &str) -> LoroList {
    let container = map
        .get(key)
        .unwrap_or_else(|| panic!("missing nested container {key}"))
        .into_container()
        .unwrap_or_else(|_| panic!("{key} is not a container"));
    LoroList::try_from_container(container).expect("nested list container")
}

fn nested_movable_list(map: &LoroMap, key: &str) -> LoroMovableList {
    let container = map
        .get(key)
        .unwrap_or_else(|| panic!("missing nested container {key}"))
        .into_container()
        .unwrap_or_else(|_| panic!("{key} is not a container"));
    LoroMovableList::try_from_container(container).expect("nested movable list container")
}

fn build_snapshot_doc() -> LoroResult<(
    LoroDoc,
    LoroText,
    LoroList,
    LoroMovableList,
    LoroTree,
    Frontiers,
    Frontiers,
)> {
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;

    let root = doc.get_map("root");
    root.insert("title", "alpha")?;
    root.insert("flag", true)?;
    root.insert("bytes", vec![1u8, 2, 3, 4])?;

    let text = root.insert_container("body", LoroText::new())?;
    text.insert(0, "Hello")?;
    text.mark(0..5, "bold", true)?;

    let list = root.insert_container("items", LoroList::new())?;
    list.insert(0, "seed")?;
    list.insert(1, "branch")?;

    let movable = root.insert_container("order", LoroMovableList::new())?;
    movable.push("draft")?;
    movable.push("published")?;
    movable.mov(0, 1)?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let tree_root = tree.create(TreeParentId::Root)?;
    tree.get_meta(tree_root)?.insert("kind", "root")?;

    doc.commit_with(CommitOptions::default().timestamp(11));
    let v1 = doc.state_frontiers();

    text.insert(text.len_unicode(), "!")?;
    list.push("tail")?;
    movable.set(0, "drafted")?;
    let tree_child = tree.create(tree_root)?;
    tree.get_meta(tree_child)?.insert("kind", "child")?;
    doc.commit_with(CommitOptions::default().timestamp(22));
    let v2 = doc.state_frontiers();

    Ok((doc, text, list, movable, tree, v1, v2))
}

#[test]
fn kv_store_trait_and_mem_store_roundtrip_sorted_keys_and_tombstones() -> anyhow::Result<()> {
    let mut btree = BTreeMap::new();
    KvStore::set(&mut btree, b"a", vec![1].into());
    KvStore::set(&mut btree, b"aa", vec![2].into());
    KvStore::set(&mut btree, b"ab", vec![3].into());
    KvStore::set(&mut btree, b"b", vec![4].into());

    assert_eq!(KvStore::get(&btree, b"aa"), Some(vec![2].into()));
    assert!(KvStore::contains_key(&btree, b"ab"));
    assert_eq!(KvStore::len(&btree), 4);
    assert!(!KvStore::is_empty(&btree));
    assert!(KvStore::compare_and_swap(
        &mut btree,
        b"aa",
        Some(vec![2].into()),
        vec![22].into()
    ));
    assert!(!KvStore::compare_and_swap(
        &mut btree,
        b"aa",
        Some(vec![2].into()),
        vec![23].into()
    ));
    assert_eq!(KvStore::remove(&mut btree, b"ab"), Some(vec![3].into()));

    assert_eq!(
        collect_scan(&btree, Unbounded, Unbounded),
        vec![
            (b"a".to_vec(), vec![1]),
            (b"aa".to_vec(), vec![22]),
            (b"b".to_vec(), vec![4]),
        ]
    );
    assert_eq!(
        collect_scan_rev(&btree, Included(b"a"), Included(b"b")),
        vec![
            (b"b".to_vec(), vec![4]),
            (b"aa".to_vec(), vec![22]),
            (b"a".to_vec(), vec![1]),
        ]
    );

    let exported = KvStore::export_all(&mut btree);
    let mut imported = BTreeMap::new();
    KvStore::import_all(&mut imported, exported.clone()).map_err(anyhow::Error::msg)?;
    assert_eq!(
        collect_scan(&imported, Unbounded, Unbounded),
        collect_scan(&btree, Unbounded, Unbounded)
    );
    assert_eq!(KvStore::get(&imported, b"aa"), Some(vec![22].into()));
    assert_eq!(KvStore::get(&imported, b"ab"), None);

    let mut mem = MemKvStore::new(MemKvConfig::default());
    KvStore::set(&mut mem, b"a", vec![1].into());
    KvStore::set(&mut mem, b"aa", vec![2].into());
    KvStore::set(&mut mem, b"ab", vec![3].into());
    KvStore::set(&mut mem, b"b", vec![4].into());
    KvStore::set(&mut mem, b"huge", vec![7u8; 100_000].into());
    let first_export = KvStore::export_all(&mut mem);

    KvStore::set(&mut mem, b"aa", vec![22].into());
    assert_eq!(KvStore::remove(&mut mem, b"ab"), Some(vec![3].into()));
    KvStore::set(&mut mem, b"c", vec![5].into());
    assert!(KvStore::compare_and_swap(
        &mut mem,
        b"c",
        Some(vec![5].into()),
        vec![6].into()
    ));
    assert_eq!(KvStore::get(&mem, b"huge"), Some(vec![7u8; 100_000].into()));
    assert_eq!(KvStore::len(&mem), 5);

    let mut imported_first = MemKvStore::new(MemKvConfig::default());
    KvStore::import_all(&mut imported_first, first_export).map_err(anyhow::Error::msg)?;
    assert_eq!(
        collect_scan(&imported_first, Unbounded, Unbounded),
        vec![
            (b"a".to_vec(), vec![1]),
            (b"aa".to_vec(), vec![2]),
            (b"ab".to_vec(), vec![3]),
            (b"b".to_vec(), vec![4]),
            (b"huge".to_vec(), vec![7u8; 100_000]),
        ]
    );

    let second_export = KvStore::export_all(&mut mem);
    let mut imported_second = MemKvStore::new(MemKvConfig::default());
    KvStore::import_all(&mut imported_second, second_export).map_err(anyhow::Error::msg)?;

    assert_eq!(
        collect_scan(&imported_second, Unbounded, Unbounded),
        collect_scan(&mem, Unbounded, Unbounded)
    );
    assert_eq!(
        collect_scan_rev(&imported_second, Included(b"a"), Included(b"huge")),
        collect_scan_rev(&mem, Included(b"a"), Included(b"huge"))
    );
    assert_eq!(KvStore::get(&imported_second, b"ab"), None);
    assert_eq!(
        KvStore::get(&imported_second, b"huge"),
        Some(vec![7u8; 100_000].into())
    );

    Ok(())
}

#[test]
fn storage_blobs_and_json_schema_roundtrip_state_and_metadata() -> anyhow::Result<()> {
    let (doc, _text, _list, _movable, _tree, v1, _v2) = build_snapshot_doc()?;
    let peer = doc.peer_id();
    let end_counter = *doc.oplog_vv().get(&peer).unwrap();

    let shallow = doc.export(ExportMode::shallow_snapshot(&v1))?;
    let state_only = doc.export(ExportMode::state_only(Some(&v1)))?;
    let snapshot_at = doc.export(ExportMode::snapshot_at(&v1))?;
    let updates = doc.export(ExportMode::updates(&VersionVector::default()))?;
    let updates_range = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(
        peer,
        0,
        end_counter,
    )]))?;

    let expected_at_v1 = doc.fork_at(&v1)?;
    let at_v1_json = deep_json(&expected_at_v1);

    let shallow_meta = LoroDoc::decode_import_blob_meta(&shallow, true)?;
    assert_eq!(shallow_meta.mode, EncodedBlobMode::ShallowSnapshot);
    assert_eq!(shallow_meta.start_frontiers, v1);
    assert_eq!(shallow_meta.start_timestamp, 11);

    let state_only_meta = LoroDoc::decode_import_blob_meta(&state_only, true)?;
    assert_eq!(state_only_meta.mode, EncodedBlobMode::ShallowSnapshot);
    assert_eq!(state_only_meta.start_frontiers, v1);

    let snapshot_at_meta = LoroDoc::decode_import_blob_meta(&snapshot_at, true)?;
    assert_eq!(snapshot_at_meta.mode, EncodedBlobMode::Snapshot);

    let updates_meta = LoroDoc::decode_import_blob_meta(&updates, true)?;
    assert_eq!(updates_meta.mode, EncodedBlobMode::Updates);
    assert_eq!(updates_meta.partial_end_vv, doc.oplog_vv());

    let updates_range_meta = LoroDoc::decode_import_blob_meta(&updates_range, true)?;
    assert_eq!(updates_range_meta.mode, EncodedBlobMode::Updates);
    assert_eq!(updates_range_meta.partial_end_vv, doc.oplog_vv());

    let snapshot_source = LoroDoc::new();
    snapshot_source.set_peer_id(41)?;
    let snapshot_root = snapshot_source.get_map("snapshot");
    snapshot_root.insert("title", "snapshot")?;
    snapshot_root.insert("flag", true)?;
    let snapshot_text = snapshot_root.insert_container("body", LoroText::new())?;
    snapshot_text.insert(0, "Hello snapshot")?;
    snapshot_source.commit_with(CommitOptions::default().timestamp(77));

    let snapshot = snapshot_source.export(ExportMode::Snapshot)?;
    let snapshot_meta = LoroDoc::decode_import_blob_meta(&snapshot, true)?;
    assert_eq!(snapshot_meta.mode, EncodedBlobMode::Snapshot);
    assert!(snapshot_meta.start_frontiers.is_empty());

    let snapshot_doc = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(deep_json(&snapshot_doc), deep_json(&snapshot_source));
    let shallow_from_snapshot = LoroDoc::from_snapshot(&shallow)?;
    assert!(shallow_from_snapshot.is_shallow());
    assert_eq!(shallow_from_snapshot.shallow_since_frontiers(), v1);
    let state_only_from_snapshot = LoroDoc::from_snapshot(&state_only)?;
    assert!(state_only_from_snapshot.is_shallow());
    assert_eq!(state_only_from_snapshot.shallow_since_frontiers(), v1);
    assert!(LoroDoc::from_snapshot(&updates).is_err());
    let mut corrupted_snapshot = snapshot.clone();
    corrupted_snapshot.truncate(corrupted_snapshot.len().saturating_sub(1));
    assert!(LoroDoc::decode_import_blob_meta(&corrupted_snapshot, true).is_err());
    assert!(LoroDoc::from_snapshot(&corrupted_snapshot).is_err());

    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow)?;
    assert!(shallow_doc.is_shallow());
    assert_eq!(shallow_doc.shallow_since_frontiers(), v1);

    let state_only_doc = LoroDoc::new();
    state_only_doc.import(&state_only)?;
    assert_eq!(deep_json(&state_only_doc), at_v1_json);

    let snapshot_at_doc = LoroDoc::new();
    snapshot_at_doc.import(&snapshot_at)?;
    assert_eq!(deep_json(&snapshot_at_doc), at_v1_json);

    let updates_doc = LoroDoc::new();
    updates_doc.import(&updates)?;
    assert_eq!(deep_json(&updates_doc), deep_json(&doc));

    let updates_range_doc = LoroDoc::new();
    updates_range_doc.import(&updates_range)?;
    assert_eq!(deep_json(&updates_range_doc), deep_json(&doc));

    let source = LoroDoc::new();
    source.set_peer_id(31)?;
    let root = source.get_map("root");
    root.insert("title", "seed")?;
    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello")?;
    let items = root.insert_container("items", LoroList::new())?;
    items.insert(0, 1)?;
    let order = root.insert_container("order", LoroMovableList::new())?;
    order.push("a")?;
    let tree = source.get_tree("tree");
    tree.enable_fractional_index(0);
    let tree_root = tree.create(TreeParentId::Root)?;
    tree.get_meta(tree_root)?.insert("kind", "root")?;
    source.commit_with(CommitOptions::default().timestamp(100));

    let remote = LoroDoc::new();
    remote.set_peer_id(32)?;
    remote.import(&source.export(ExportMode::Snapshot)?)?;
    let remote_root = remote.get_map("root");
    nested_text(&remote_root, "body").insert(0, "remote ")?;
    nested_list(&remote_root, "items").push(2)?;
    nested_movable_list(&remote_root, "order").push("remote")?;
    remote
        .get_tree("tree")
        .get_meta(tree_root)?
        .insert("remote", true)?;
    remote.commit_with(CommitOptions::default().timestamp(200));

    source.import(&remote.export(ExportMode::all_updates())?)?;

    let start = VersionVector::default();
    let end = source.oplog_vv();
    let compressed = source.export_json_updates(&start, &end);
    let uncompressed = source.export_json_updates_without_peer_compression(&start, &end);

    assert!(compressed.peers.is_some());
    assert!(uncompressed.peers.is_none());

    let mut op_kinds = HashSet::new();
    for change in &compressed.changes {
        for op in &change.ops {
            match &op.content {
                JsonOpContent::List(_) => {
                    op_kinds.insert("list");
                }
                JsonOpContent::MovableList(_) => {
                    op_kinds.insert("movable_list");
                }
                JsonOpContent::Map(_) => {
                    op_kinds.insert("map");
                }
                JsonOpContent::Text(_) => {
                    op_kinds.insert("text");
                }
                JsonOpContent::Tree(_) => {
                    op_kinds.insert("tree");
                }
                JsonOpContent::Future(_) => {
                    op_kinds.insert("future");
                }
            }
        }
    }
    assert!(op_kinds.contains("map"));
    assert!(op_kinds.contains("text"));
    assert!(op_kinds.contains("list"));
    assert!(op_kinds.contains("movable_list"));
    assert!(op_kinds.contains("tree"));

    let compressed_json = serde_json::to_string(&compressed)?;
    let parsed_from_str: JsonSchema = compressed_json.as_str().try_into()?;
    let parsed_from_string: JsonSchema = compressed_json.clone().try_into()?;
    assert_eq!(parsed_from_str.changes.len(), compressed.changes.len());
    assert_eq!(parsed_from_string.changes.len(), compressed.changes.len());

    let compressed_doc = LoroDoc::new();
    compressed_doc.import_json_updates(compressed.clone())?;
    assert_eq!(deep_json(&compressed_doc), deep_json(&source));

    let compressed_doc_from_string = LoroDoc::new();
    compressed_doc_from_string.import_json_updates(compressed_json.clone())?;
    assert_eq!(deep_json(&compressed_doc_from_string), deep_json(&source));

    let uncompressed_doc = LoroDoc::new();
    uncompressed_doc.import_json_updates(uncompressed.clone())?;
    assert_eq!(deep_json(&uncompressed_doc), deep_json(&source));

    let uncompressed_json = serde_json::to_string(&uncompressed)?;
    let uncompressed_doc_from_string = LoroDoc::new();
    uncompressed_doc_from_string.import_json_updates(uncompressed_json.clone())?;
    assert_eq!(deep_json(&uncompressed_doc_from_string), deep_json(&source));

    let changes = source.export_json_in_id_span(IdSpan::new(
        source.peer_id(),
        0,
        *end.get(&source.peer_id()).unwrap(),
    ));
    assert!(!changes.is_empty());
    let encoded_changes = serde_json::to_value(&changes)?;
    assert!(encoded_changes.is_array());

    Ok(())
}
