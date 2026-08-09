use loro::{
    ContainerTrait, ExportMode, JsonListOp, JsonOpContent, LoroDoc, LoroList, LoroMovableList,
    LoroResult, LoroText, ToJson, TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::Value;

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn nested_body(doc: &LoroDoc) -> LoroText {
    LoroText::try_from_container(
        doc.get_map("root")
            .get("body")
            .expect("body should exist")
            .into_container()
            .expect("body should be a text container"),
    )
    .expect("body should be a text container")
}

#[test]
fn all_updates_snapshot_and_incremental_updates_roundtrip_state() -> LoroResult<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;

    let root = source.get_map("root");
    root.insert("title", "Spec")?;

    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello")?;

    let items = root.insert_container("items", LoroList::new())?;
    items.insert(0, "first")?;

    let order = root.insert_container("order", LoroMovableList::new())?;
    order.push("todo")?;

    let tree = source.get_tree("tree");
    let tree_root = tree.create(TreeParentId::Root)?;
    tree.get_meta(tree_root)?.insert("kind", "root")?;

    source.commit();
    let base_vv = source.oplog_vv();

    body.insert(body.len_unicode(), " world")?;
    root.insert("done", true)?;
    items.push("second")?;
    order.push("doing")?;
    source.commit();

    let all_updates = source.export(ExportMode::all_updates())?;
    let incremental_updates = source.export(ExportMode::updates(&base_vv))?;
    let snapshot = source.export(ExportMode::Snapshot)?;

    let from_all_updates = LoroDoc::new();
    from_all_updates.import(&all_updates)?;
    assert_eq!(deep_json(&from_all_updates), deep_json(&source));

    let from_snapshot = LoroDoc::new();
    from_snapshot.import(&snapshot)?;
    assert_eq!(deep_json(&from_snapshot), deep_json(&source));

    let replay = LoroDoc::new();
    let status = replay.import_batch(&[
        incremental_updates.clone(),
        incremental_updates,
        snapshot.clone(),
    ])?;
    assert!(status.pending.is_none());
    assert_eq!(deep_json(&replay), deep_json(&source));

    Ok(())
}

#[test]
fn detached_checkout_can_receive_remote_updates_and_return_to_latest() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(10)?;

    let text = doc.get_text("text");
    text.insert(0, "Hello")?;
    doc.commit();
    let v1 = doc.state_frontiers();

    text.insert(5, " world")?;
    doc.commit();

    doc.checkout(&v1)?;
    assert!(doc.is_detached());
    assert_eq!(text.to_string(), "Hello");

    let remote = LoroDoc::new();
    remote.set_peer_id(11)?;
    remote.import(&doc.export(ExportMode::all_updates())?)?;
    remote
        .get_text("text")
        .insert(remote.get_text("text").len_unicode(), "!")?;
    remote.commit();

    doc.import(&remote.export(ExportMode::all_updates())?)?;
    doc.checkout_to_latest();

    assert_eq!(doc.get_text("text").to_string(), "Hello world!");
    assert_eq!(deep_json(&doc), deep_json(&remote));

    Ok(())
}

#[test]
fn concurrent_peers_converge_via_incremental_sync_and_snapshot_roundtrip() -> LoroResult<()> {
    let seed = LoroDoc::new();
    seed.set_peer_id(1)?;

    let root = seed.get_map("root");
    root.insert("title", "base")?;

    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello")?;

    let items = root.insert_container("items", LoroList::new())?;
    items.insert(0, "seed")?;

    let order = root.insert_container("order", LoroMovableList::new())?;
    order.push("seed")?;

    let tree = seed.get_tree("tree");
    let tree_root = tree.create(TreeParentId::Root)?;
    tree.get_meta(tree_root)?.insert("kind", "root")?;

    seed.commit();
    let base_snapshot = seed.export(ExportMode::Snapshot)?;
    let base_vv = seed.oplog_vv();

    let a = LoroDoc::new();
    a.set_peer_id(2)?;
    a.import(&base_snapshot)?;

    let b = LoroDoc::new();
    b.set_peer_id(3)?;
    b.import(&base_snapshot)?;

    seed.get_text("seed_text").insert(0, "seed")?;
    seed.get_map("root").insert("seed_only", true)?;
    seed.commit();

    nested_body(&a).insert(0, "A: ")?;
    a.get_list("items").push("a-only")?;
    a.commit();

    let b_body = nested_body(&b);
    b_body.insert(b_body.len_unicode(), " from B")?;
    b.get_movable_list("order").push("b-only")?;
    b.commit();

    let seed_updates = seed.export(ExportMode::updates(&base_vv))?;
    let a_updates = a.export(ExportMode::updates(&base_vv))?;
    let b_updates = b.export(ExportMode::updates(&base_vv))?;

    seed.import(&a_updates)?;
    seed.import(&b_updates)?;
    a.import(&seed_updates)?;
    a.import(&b_updates)?;
    b.import(&seed_updates)?;
    b.import(&a_updates)?;

    assert_eq!(deep_json(&seed), deep_json(&a));
    assert_eq!(deep_json(&a), deep_json(&b));

    let snapshot = seed.export(ExportMode::Snapshot)?;
    let replay = LoroDoc::new();
    replay.import(&snapshot)?;
    assert_eq!(deep_json(&replay), deep_json(&seed));

    Ok(())
}

/// `import_batch` force-detaches the doc while it feeds blobs into the OpLog and
/// reattaches once at the end. A blob whose ops are only rejected when they reach
/// `DocState` used to make that reattach panic, leaving the document detached: every
/// later import stopped showing up and local edits branched off a stale version.
/// The batch must fail as a unit instead, leaving an attached, unchanged document.
#[test]
fn import_batch_failure_leaves_doc_attached_and_unchanged() -> LoroResult<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    source.get_text("text").insert(0, "hello")?;
    source.get_list("list").push("seed")?;
    source.commit();
    let snapshot = source.export(ExportMode::Snapshot)?;
    let base_vv = source.oplog_vv();

    // A chain of updates from a second peer. Sorted newest-first, everything but the
    // oldest blob sits in `pending_changes` until its deps land.
    let peer2 = LoroDoc::new();
    peer2.set_peer_id(2)?;
    peer2.import(&snapshot)?;
    let mut blobs = Vec::new();
    let mut from = base_vv;
    for i in 0..4 {
        peer2.get_map("map").insert(&format!("k{i}"), i)?;
        peer2.commit();
        blobs.push(peer2.export(ExportMode::updates(&from))?);
        from = peer2.oplog_vv();
    }
    blobs.reverse();
    blobs.insert(0, update_with_out_of_bounds_list_insert(9)?);

    let target = LoroDoc::new();
    target.set_peer_id(3)?;
    target.import(&snapshot)?;
    let vv_before = target.oplog_vv();
    let value_before = deep_json(&target);

    let err = target
        .import_batch(&blobs)
        .expect_err("an op the state rejects must fail the batch");
    assert!(
        err.to_string().contains("list diff"),
        "unexpected error: {err:?}"
    );

    assert!(
        !target.is_detached(),
        "import_batch must never exit in detached mode"
    );
    assert_eq!(target.oplog_vv(), vv_before);
    assert_eq!(deep_json(&target), value_before);

    // The doc is still usable, and a batch without the bad blob still applies.
    target.get_text("text").insert(0, "ok: ")?;
    target.commit();
    target.import_batch(&blobs[1..])?;
    assert!(!target.is_detached());
    assert_eq!(
        target.get_map("map").get("k3").is_some(),
        true,
        "the good blobs should have been applied"
    );

    Ok(())
}

/// Binary updates carrying a list insert far past the end of the list. It decodes into
/// the OpLog fine and is only rejected by `DocState` validation.
fn update_with_out_of_bounds_list_insert(peer: u64) -> LoroResult<Vec<u8>> {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_list("list").insert(0, "seed")?;
    doc.get_list("list").insert(1, "tail")?;
    doc.commit();

    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let last_op = json
        .changes
        .last_mut()
        .expect("one change")
        .ops
        .last_mut()
        .expect("one op");
    match &mut last_op.content {
        JsonOpContent::List(JsonListOp::Insert { pos, .. }) => *pos = 1_000,
        other => panic!("expected a list insert op, got {other:?}"),
    }

    // Detached, the change only enters the OpLog, so the malformed op survives export.
    let carrier = LoroDoc::new();
    carrier.detach();
    carrier.import_json_updates(serde_json::to_string(&json).unwrap())?;
    Ok(carrier.export(ExportMode::all_updates()).unwrap())
}
