use loro::{ContainerTrait, Frontiers, LoroDoc, ID};
use std::sync::{atomic::AtomicBool, Arc};

#[test]
fn test_root_subscription_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "A")?;
    doc.commit();

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
    }));

    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;

    doc.get_text("text").insert(1, "B")?;
    doc.commit();

    assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn test_container_subscription_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "A")?;
    doc.commit();

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    let _sub = doc.subscribe(
        &text.id(),
        Arc::new(move |_| {
            called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        }),
    );

    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;

    text.insert(1, "B")?;
    doc.commit();

    assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn test_local_update_subscription_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "A")?;
    doc.commit();

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    let _sub = doc.subscribe_local_update(Box::new(move |_| {
        called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        true
    }));

    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;

    doc.get_text("text").insert(1, "B")?;
    doc.commit();

    assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn test_peer_id_change_subscription_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    let _sub = doc.subscribe_peer_id_change(Box::new(move |_| {
        called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        true
    }));

    doc.get_text("text").insert(0, "A")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;

    doc.set_peer_id(2)?;

    assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn test_replace_on_empty_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;
    assert!(doc.get_deep_value().as_map().unwrap().is_empty());
    Ok(())
}

#[test]
fn test_replace_on_already_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "A")?;
    doc.commit();
    let f1 = doc.oplog_frontiers();
    doc.replace_with_shallow(&f1)?;

    doc.get_text("text").insert(1, "B")?;
    doc.commit();
    let f2 = doc.oplog_frontiers();
    doc.replace_with_shallow(&f2)?;

    assert_eq!(doc.get_text("text").to_string(), "AB");
    Ok(())
}

#[test]
fn test_replace_with_invalid_frontiers() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "A")?;
    doc.commit();

    let invalid_frontiers = Frontiers::from(ID::new(123, 456));
    let result = doc.replace_with_shallow(&invalid_frontiers);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_replace_with_deleted_containers() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let map = doc.get_map("map");
    let list = map.insert_container("list", loro::LoroList::new())?;
    list.insert(0, 1)?;
    doc.commit();
    map.delete("list")?;
    doc.commit();
    let f2 = doc.oplog_frontiers();

    doc.replace_with_shallow(&f2)?;
    assert!(doc.get_map("map").get("list").is_none());
    Ok(())
}

#[test]
fn test_replace_when_detached() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "A")?;
    doc.commit();
    let f1 = doc.oplog_frontiers();
    doc.get_text("text").insert(1, "B")?;
    doc.commit();

    doc.checkout(&f1)?;
    assert!(doc.is_detached());

    // Replace with shallow at f1
    doc.replace_with_shallow(&f1)?;

    // Should still be detached?
    // `replace_with_shallow` preserves detached state.
    assert!(doc.is_detached());
    assert_eq!(doc.get_text("text").to_string(), "A");

    Ok(())
}

#[test]
fn test_peer_id_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(123)?;
    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;
    assert_eq!(doc.peer_id(), 123);
    Ok(())
}

#[test]
fn test_auto_commit_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "A")?; // auto commit
    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;
    doc.get_text("text").insert(1, "B")?; // should auto commit
    assert_eq!(doc.get_text("text").to_string(), "AB");
    assert_ne!(doc.oplog_frontiers(), frontiers);
    Ok(())
}

#[test]
fn test_detached_flag_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "A")?;
    doc.commit();
    let f = doc.oplog_frontiers();
    doc.detach();
    assert!(doc.is_detached());
    doc.replace_with_shallow(&f)?;
    assert!(doc.is_detached());
    Ok(())
}

#[test]
fn test_config_preservation() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_change_merge_interval(123);
    doc.set_record_timestamp(true);
    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;
    assert_eq!(doc.config().merge_interval(), 123);
    assert!(doc.config().record_timestamp());
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_replace_with_shallow_memory_leak() {
    use dev_utils::ByteSize;

    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");

    // Initial population
    for _ in 0..100 {
        text.insert(0, "a").unwrap();
        doc.commit();
    }

    let base_mem = dev_utils::get_mem_usage();

    for _ in 0..100 {
        text.insert(0, "b").unwrap();
        doc.commit();
        let f = doc.oplog_frontiers();
        doc.replace_with_shallow(&f).unwrap();
    }

    let current_mem = dev_utils::get_mem_usage();
    assert!(
        current_mem < base_mem + ByteSize(5 * 1024 * 1024),
        "Memory grew too much: {:?} -> {:?}",
        base_mem,
        current_mem
    );
}

#[test]
fn test_handler_validity_after_replace() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "A")?;
    doc.commit();

    let map = doc.get_map("map");
    map.insert("key", "value")?;
    doc.commit();

    let list = doc.get_list("list");
    list.insert(0, 1)?;
    doc.commit();

    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;

    // Use old handlers
    text.insert(1, "B")?;
    map.insert("key2", "value2")?;
    list.insert(1, 2)?;
    doc.commit();

    assert_eq!(doc.get_text("text").to_string(), "AB");
    assert_eq!(
        doc.get_map("map")
            .get("key2")
            .unwrap()
            .into_value()
            .unwrap()
            .as_string()
            .unwrap()
            .as_str(),
        "value2"
    );
    assert_eq!(
        *doc.get_list("list")
            .get(1)
            .unwrap()
            .into_value()
            .unwrap()
            .as_i64()
            .unwrap(),
        2
    );

    Ok(())
}

#[test]
fn test_parent_resolver_after_replace() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let map = doc.get_map("map");
    let list = map.insert_container("list", loro::LoroList::new())?;
    let text = list.insert_container(0, loro::LoroText::new())?;
    text.insert(0, "hello")?;
    doc.commit();

    let frontiers = doc.oplog_frontiers();
    doc.replace_with_shallow(&frontiers)?;

    // Verify parent relationships
    let text_id = text.id();
    let list_id = list.id();
    let map_id = map.id();

    // We can't directly access parent resolver easily from public API,
    // but we can check if we can access the container and if operations work,
    // which implies parent resolution works for path generation etc.

    // Or we can use `get_path_to_container`
    let path = doc.get_path_to_container(&text_id).unwrap();
    // Path should be map -> list -> text
    assert_eq!(path.len(), 3); // (map, "map"), (list, "list"), (text, 0)
    assert_eq!(path[0].0, map_id);
    assert_eq!(path[1].0, list_id);
    assert_eq!(path[2].0, text_id);

    Ok(())
}

#[test]
fn test_concurrent_read_during_replace() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let doc = Arc::new(LoroDoc::new());
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    for i in 0..100 {
        text.insert(0, &i.to_string()).unwrap();
        doc.commit();
    }

    let barrier = Arc::new(Barrier::new(2));
    let doc_clone = doc.clone();
    let barrier_clone = barrier.clone();

    let handle = thread::spawn(move || {
        barrier_clone.wait();
        for _ in 0..100 {
            let _ = doc_clone.get_text("text").to_string();
        }
    });

    barrier.wait();
    let f = doc.oplog_frontiers();
    doc.replace_with_shallow(&f).unwrap();

    handle.join().unwrap();
    assert_eq!(doc.get_text("text").len_unicode(), text.len_unicode());
}

#[test]
fn test_concurrent_write_during_replace() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let doc = Arc::new(LoroDoc::new());
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "Start").unwrap();
    doc.commit();

    let barrier = Arc::new(Barrier::new(2));
    let doc_clone = doc.clone();
    let barrier_clone = barrier.clone();

    let handle = thread::spawn(move || {
        barrier_clone.wait();
        for i in 0..100 {
            // We expect some writes might fail if replace_with_shallow is holding lock?
            // Or they succeed.
            // LoroDoc uses internal locking, so it should be safe.
            let _ = doc_clone.get_text("text").insert(0, &i.to_string());
        }
    });

    barrier.wait();
    let f = doc.oplog_frontiers();
    doc.replace_with_shallow(&f).unwrap();

    handle.join().unwrap();
    // We don't assert exact content because race condition determines order,
    // but it should not panic.
}
