use loro::{
    event::{Diff, ListDiffItem},
    CommitOptions, ContainerID, ContainerTrait, EventTriggerKind, ExportMode, Index, LoroDoc,
    LoroList, LoroMovableList, LoroText, LoroTree, Timestamp, ID,
};
use pretty_assertions::assert_eq;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum CapturedDiffKind {
    Map,
    Text,
    Tree,
    List { has_move: bool },
    Counter,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedContainerDiff {
    target: ContainerID,
    path: Vec<(ContainerID, Index)>,
    kind: CapturedDiffKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedEvent {
    triggered_by: EventTriggerKind,
    origin: String,
    current_target: Option<ContainerID>,
    diffs: Vec<CapturedContainerDiff>,
}

fn capture_event(event: loro::event::DiffEvent<'_>) -> CapturedEvent {
    CapturedEvent {
        triggered_by: event.triggered_by,
        origin: event.origin.to_owned(),
        current_target: event.current_target,
        diffs: event
            .events
            .into_iter()
            .map(|diff| CapturedContainerDiff {
                target: diff.target.clone(),
                path: diff.path.to_vec(),
                kind: match diff.diff {
                    Diff::Map(_) => CapturedDiffKind::Map,
                    Diff::Text(_) => CapturedDiffKind::Text,
                    Diff::Tree(_) => CapturedDiffKind::Tree,
                    Diff::Counter(_) => CapturedDiffKind::Counter,
                    Diff::List(items) => CapturedDiffKind::List {
                        has_move: items
                            .iter()
                            .any(|item| matches!(item, ListDiffItem::Insert { is_move: true, .. })),
                    },
                    Diff::Unknown => CapturedDiffKind::Unknown,
                },
            })
            .collect(),
    }
}

fn assert_diff(event: &CapturedEvent, target: &ContainerID, kind: CapturedDiffKind) {
    let diff = event
        .diffs
        .iter()
        .find(|diff| &diff.target == target)
        .unwrap_or_else(|| panic!("missing diff for target {target:?}: {event:?}"));
    assert_eq!(diff.kind, kind);
}

fn assert_path_contains(
    event: &CapturedEvent,
    target: &ContainerID,
    segment: (ContainerID, Index),
) {
    let diff = event
        .diffs
        .iter()
        .find(|diff| &diff.target == target)
        .unwrap_or_else(|| panic!("missing diff for target {target:?}: {event:?}"));
    assert!(
        diff.path.iter().any(|item| item == &segment),
        "missing path segment {segment:?} in {diff:?}"
    );
}

#[test]
fn root_and_container_subscriptions_expose_current_target_and_diff_shapes() -> loro::LoroResult<()>
{
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;

    let root = doc.get_map("root");
    let root_id = root.id();

    let text = root.insert_container("text", LoroText::new())?;
    let list = root.insert_container("list", LoroList::new())?;
    let movable_list = root.insert_container("mov", LoroMovableList::new())?;
    let tree = root.insert_container("tree", LoroTree::new())?;
    doc.commit();

    let root_events = Arc::new(Mutex::new(Vec::new()));
    let root_events_clone = Arc::clone(&root_events);
    let _root_sub = doc.subscribe_root(Arc::new(move |event| {
        root_events_clone.lock().unwrap().push(capture_event(event));
    }));
    let container_events = Arc::new(Mutex::new(Vec::new()));
    let container_events_clone = Arc::clone(&container_events);
    let _container_sub = doc.subscribe(
        &root_id,
        Arc::new(move |event| {
            container_events_clone
                .lock()
                .unwrap()
                .push(capture_event(event));
        }),
    );

    text.insert(0, "Hi")?;
    list.push(1)?;
    list.push(2)?;
    movable_list.push(10)?;
    movable_list.push(20)?;
    movable_list.mov(0, 1)?;
    let root_node = tree.create(None)?;
    let child_node = tree.create(root_node)?;
    tree.mov(child_node, None)?;
    root.insert("plain", 7)?;
    doc.commit();

    let root_event = root_events.lock().unwrap().first().cloned().unwrap();
    assert_eq!(root_event.triggered_by, EventTriggerKind::Local);
    assert_eq!(root_event.origin, "");
    assert_eq!(root_event.current_target, None);
    assert_diff(&root_event, &root_id, CapturedDiffKind::Map);
    assert_path_contains(
        &root_event,
        &root_id,
        (root_id.clone(), Index::Key("root".into())),
    );
    assert_diff(&root_event, &text.id(), CapturedDiffKind::Text);
    assert_path_contains(
        &root_event,
        &text.id(),
        (text.id(), Index::Key("text".into())),
    );
    assert_diff(
        &root_event,
        &list.id(),
        CapturedDiffKind::List { has_move: false },
    );
    assert_path_contains(
        &root_event,
        &list.id(),
        (list.id(), Index::Key("list".into())),
    );
    assert_diff(
        &root_event,
        &movable_list.id(),
        CapturedDiffKind::List { has_move: true },
    );
    assert_path_contains(
        &root_event,
        &movable_list.id(),
        (movable_list.id(), Index::Key("mov".into())),
    );
    assert_diff(&root_event, &tree.id(), CapturedDiffKind::Tree);
    assert_path_contains(
        &root_event,
        &tree.id(),
        (tree.id(), Index::Key("tree".into())),
    );

    let container_event = container_events.lock().unwrap().first().cloned().unwrap();
    assert_eq!(container_event.triggered_by, EventTriggerKind::Local);
    assert_eq!(container_event.origin, "");
    assert_eq!(container_event.current_target, Some(root_id.clone()));
    assert_diff(&container_event, &root_id, CapturedDiffKind::Map);
    assert_path_contains(
        &container_event,
        &root_id,
        (root_id.clone(), Index::Key("root".into())),
    );

    Ok(())
}

#[test]
fn root_subscription_reports_import_and_checkout_trigger_kinds() -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;

    let kinds = Arc::new(Mutex::new(Vec::<EventTriggerKind>::new()));
    let kinds_clone = Arc::clone(&kinds);
    let _sub = doc.subscribe_root(Arc::new(move |event| {
        kinds_clone.lock().unwrap().push(event.triggered_by);
    }));

    doc.get_text("text").insert(0, "a")?;
    doc.commit();
    let f1 = doc.state_frontiers();

    doc.get_text("text").insert(1, "b")?;
    doc.commit();
    doc.checkout(&f1)?;

    assert_eq!(
        kinds.lock().unwrap().as_slice(),
        &[
            EventTriggerKind::Local,
            EventTriggerKind::Local,
            EventTriggerKind::Checkout
        ]
    );

    let imported = LoroDoc::new();
    let import_kinds = Arc::new(Mutex::new(Vec::<EventTriggerKind>::new()));
    let import_kinds_clone = Arc::clone(&import_kinds);
    let _import_sub = imported.subscribe_root(Arc::new(move |event| {
        import_kinds_clone.lock().unwrap().push(event.triggered_by);
    }));
    imported.import(&doc.export(ExportMode::Snapshot)?)?;

    assert_eq!(
        import_kinds.lock().unwrap().as_slice(),
        &[EventTriggerKind::Import]
    );

    Ok(())
}

#[test]
fn pre_commit_can_rewrite_commit_metadata() -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(13)?;

    let pre_commit_payloads = Arc::new(Mutex::new(Vec::<(String, String, Timestamp)>::new()));
    let pre_commit_payloads_clone = Arc::clone(&pre_commit_payloads);
    let _pre_commit = doc.subscribe_pre_commit(Box::new(move |payload| {
        pre_commit_payloads_clone.lock().unwrap().push((
            payload.origin.clone(),
            payload.change_meta.message().to_owned(),
            payload.change_meta.timestamp(),
        ));
        payload.modifier.set_message("hooked").set_timestamp(4242);
        true
    }));

    let root_origins = Arc::new(Mutex::new(Vec::<String>::new()));
    let root_origins_clone = Arc::clone(&root_origins);
    let _root = doc.subscribe_root(Arc::new(move |event| {
        root_origins_clone
            .lock()
            .unwrap()
            .push(event.origin.to_owned());
    }));

    doc.set_next_commit_options(
        CommitOptions::new()
            .origin("ui")
            .commit_msg("from options")
            .timestamp(7),
    );
    doc.get_text("text").insert(0, "x")?;
    doc.commit();

    let first_change = doc.get_change(ID::new(13, 0)).unwrap();
    assert_eq!(first_change.message(), "hooked");
    assert_eq!(first_change.timestamp(), 4242);
    assert_eq!(
        pre_commit_payloads.lock().unwrap().as_slice(),
        &[("ui".to_string(), "from options".to_string(), 7)]
    );
    assert_eq!(root_origins.lock().unwrap().as_slice(), &["ui".to_string()]);

    Ok(())
}

#[test]
fn empty_commits_do_not_carry_explicit_options_but_barriers_preserve_message_and_timestamp(
) -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(17)?;

    let root_origins = Arc::new(Mutex::new(Vec::<String>::new()));
    let root_origins_clone = Arc::clone(&root_origins);
    let _root = doc.subscribe_root(Arc::new(move |event| {
        root_origins_clone
            .lock()
            .unwrap()
            .push(event.origin.to_owned());
    }));

    doc.commit_with(
        CommitOptions::new()
            .origin("discard")
            .commit_msg("discard")
            .timestamp(11),
    );
    doc.get_text("text").insert(0, "a")?;
    doc.commit();

    let first_change = doc.get_change(ID::new(17, 0)).unwrap();
    assert_eq!(first_change.message(), "");
    assert_eq!(root_origins.lock().unwrap().as_slice(), &["".to_string()]);

    doc.set_next_commit_options(CommitOptions::new().commit_msg("carry").timestamp(99));
    let _ = doc.export(ExportMode::all_updates())?;
    doc.get_text("text").insert(1, "b")?;
    doc.commit();

    let second_change = doc.get_change(ID::new(17, 1)).unwrap();
    assert_eq!(second_change.message(), "carry");
    assert_eq!(second_change.timestamp(), 99);
    assert_eq!(
        root_origins.lock().unwrap().as_slice(),
        &["".to_string(), "".to_string()]
    );

    doc.set_record_timestamp(true);
    doc.get_text("text").insert(2, "c")?;
    doc.commit();

    let third_change = doc.get_change(ID::new(17, 2)).unwrap();
    assert!(third_change.timestamp() > 100_000);

    Ok(())
}

#[test]
fn local_update_peer_id_and_first_commit_subscriptions_follow_their_contracts(
) -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(21)?;

    let local_update_count = Arc::new(AtomicUsize::new(0));
    let local_update_count_clone = Arc::clone(&local_update_count);
    let local_updates = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let local_updates_clone = Arc::clone(&local_updates);
    let _local_update = doc.subscribe_local_update(Box::new(move |bytes| {
        local_update_count_clone.fetch_add(1, Ordering::SeqCst);
        local_updates_clone.lock().unwrap().push(bytes.clone());
        false
    }));

    let peer_changes = Arc::new(Mutex::new(Vec::<ID>::new()));
    let peer_changes_clone = Arc::clone(&peer_changes);
    let _peer_id = doc.subscribe_peer_id_change(Box::new(move |id| {
        peer_changes_clone.lock().unwrap().push(*id);
        false
    }));

    let first_commit_peers = Arc::new(Mutex::new(Vec::<u64>::new()));
    let first_commit_peers_clone = Arc::clone(&first_commit_peers);
    let _first_commit = doc.subscribe_first_commit_from_peer(Box::new(move |payload| {
        first_commit_peers_clone.lock().unwrap().push(payload.peer);
        true
    }));

    doc.get_text("text").insert(0, "hello")?;
    doc.commit();
    assert_eq!(local_update_count.load(Ordering::SeqCst), 1);
    assert_eq!(first_commit_peers.lock().unwrap().as_slice(), &[21]);

    let receiver = LoroDoc::new();
    receiver.import(&local_updates.lock().unwrap()[0])?;
    assert_eq!(receiver.get_text("text").to_string(), "hello");

    doc.set_peer_id(22)?;
    assert_eq!(peer_changes.lock().unwrap().as_slice(), &[ID::new(22, 0)]);

    doc.get_text("text").insert(5, " world")?;
    doc.commit();
    assert_eq!(local_update_count.load(Ordering::SeqCst), 1);
    assert_eq!(first_commit_peers.lock().unwrap().as_slice(), &[21, 22]);

    doc.set_peer_id(23)?;
    assert_eq!(peer_changes.lock().unwrap().as_slice(), &[ID::new(22, 0)]);

    Ok(())
}

#[test]
fn subscriptions_auto_unsubscribe_and_drop_follow_contract() -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(29)?;
    let text = doc.get_text("text");

    let one_shot_roots = Arc::new(AtomicUsize::new(0));
    let one_shot_roots_clone = Arc::clone(&one_shot_roots);
    let _root = doc.subscribe_root(Arc::new(move |_| {
        one_shot_roots_clone.fetch_add(1, Ordering::SeqCst);
    }));
    let one_shot_pre_commit = Arc::new(AtomicUsize::new(0));
    let one_shot_pre_commit_clone = Arc::clone(&one_shot_pre_commit);
    let _pre_commit = doc.subscribe_pre_commit(Box::new(move |_| {
        one_shot_pre_commit_clone.fetch_add(1, Ordering::SeqCst);
        false
    }));

    text.insert(0, "a")?;
    doc.commit();
    text.insert(1, "b")?;
    doc.commit();
    assert_eq!(one_shot_roots.load(Ordering::SeqCst), 2);
    assert_eq!(one_shot_pre_commit.load(Ordering::SeqCst), 1);

    let container_events = Arc::new(AtomicUsize::new(0));
    let container_events_clone = Arc::clone(&container_events);
    let sub = doc.subscribe(
        &text.id(),
        Arc::new(move |_| {
            container_events_clone.fetch_add(1, Ordering::SeqCst);
        }),
    );
    text.insert(2, "c")?;
    doc.commit();
    assert_eq!(container_events.load(Ordering::SeqCst), 1);

    drop(sub);
    text.insert(3, "d")?;
    doc.commit();
    assert_eq!(container_events.load(Ordering::SeqCst), 1);

    Ok(())
}

#[test]
fn subscription_callbacks_can_queue_recursive_document_changes() -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(30)?;
    let text = doc.get_text("text");

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = Arc::clone(&calls);
    let doc_clone = doc.clone();
    let text_clone = text.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        let call_index = calls_clone.fetch_add(1, Ordering::SeqCst);
        if call_index < 2 {
            let len = text_clone.len_unicode();
            text_clone.insert(len, "x").unwrap();
            doc_clone.commit();
        }
    }));

    text.insert(0, "x")?;
    doc.commit();

    assert_eq!(text.to_string(), "xxx");
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    Ok(())
}

#[test]
fn detached_subscription_handle_keeps_callback_registered_for_document_lifetime(
) -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(32)?;
    let text = doc.get_text("text");

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = Arc::clone(&calls);
    let sub = doc.subscribe_root(Arc::new(move |_| {
        calls_clone.fetch_add(1, Ordering::SeqCst);
    }));
    sub.detach();

    text.insert(0, "a")?;
    doc.commit();
    text.insert(1, "b")?;
    doc.commit();

    assert_eq!(calls.load(Ordering::SeqCst), 2);

    Ok(())
}

#[test]
fn change_merge_interval_merges_adjacent_changes_with_matching_metadata() -> loro::LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(31)?;
    doc.set_change_merge_interval(1);

    doc.get_text("text").insert(0, "a")?;
    doc.commit_with(CommitOptions::new().timestamp(10));
    doc.get_text("text").insert(1, "b")?;
    doc.commit_with(CommitOptions::new().timestamp(11));

    assert_eq!(doc.len_changes(), 1);

    let change = doc.get_change(ID::new(31, 0)).unwrap();
    assert_eq!(change.len, 2);
    assert_eq!(change.message(), "");
    assert_eq!(change.timestamp(), 10);

    Ok(())
}
