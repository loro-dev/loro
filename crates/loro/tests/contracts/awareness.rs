#[allow(deprecated)]
use loro::awareness::Awareness;
use loro::{
    awareness::{EphemeralEventTrigger, EphemeralStore},
    LoroValue,
};
use pretty_assertions::assert_eq;
use std::{
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
};

type CapturedEvent = (EphemeralEventTrigger, Vec<String>, Vec<String>, Vec<String>);

fn capture_events(store: &EphemeralStore) -> (loro::Subscription, Arc<Mutex<Vec<CapturedEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let subscription = store.subscribe(Box::new(move |event| {
        events_clone.lock().unwrap().push((
            event.by,
            event.added.as_ref().clone(),
            event.updated.as_ref().clone(),
            event.removed.as_ref().clone(),
        ));
        true
    }));

    (subscription, events)
}

#[test]
fn ephemeral_store_syncs_presence_by_payloads_and_reports_events() {
    let sender = EphemeralStore::new(30_000);
    let receiver = EphemeralStore::new(30_000);

    let local_payloads = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let payloads_clone = Arc::clone(&local_payloads);
    let _local_updates = sender.subscribe_local_updates(Box::new(move |bytes| {
        payloads_clone.lock().unwrap().push(bytes.clone());
        true
    }));
    let (_receiver_events_sub, receiver_events) = capture_events(&receiver);

    sender.set("cursor", 1);
    let add_payload = local_payloads.lock().unwrap().last().unwrap().clone();
    receiver.apply(&add_payload).unwrap();
    assert_eq!(receiver.get("cursor"), Some(LoroValue::from(1)));
    assert_eq!(receiver.keys(), vec!["cursor".to_string()]);
    assert_eq!(
        receiver_events.lock().unwrap().as_slice(),
        [(
            EphemeralEventTrigger::Import,
            vec!["cursor".to_string()],
            vec![],
            vec![],
        )]
    );

    sleep(Duration::from_millis(2));
    sender.set("cursor", 2);
    let update_payload = local_payloads.lock().unwrap().last().unwrap().clone();
    receiver.apply(&update_payload).unwrap();
    assert_eq!(receiver.get("cursor"), Some(LoroValue::from(2)));
    assert_eq!(
        receiver_events.lock().unwrap().last(),
        Some(&(
            EphemeralEventTrigger::Import,
            vec![],
            vec!["cursor".to_string()],
            vec![],
        ))
    );

    sleep(Duration::from_millis(2));
    sender.delete("cursor");
    let delete_payload = local_payloads.lock().unwrap().last().unwrap().clone();
    receiver.apply(&delete_payload).unwrap();
    assert_eq!(receiver.get("cursor"), None);
    assert!(receiver.keys().is_empty());
    assert_eq!(
        receiver_events.lock().unwrap().last(),
        Some(&(
            EphemeralEventTrigger::Import,
            vec![],
            vec![],
            vec!["cursor".to_string()],
        ))
    );
}

#[test]
fn ephemeral_store_omits_expired_payloads_and_removes_them_by_timeout() {
    let store = EphemeralStore::new(5);
    let (_events_sub, events) = capture_events(&store);

    store.set("status", "online");
    assert_eq!(store.get("status"), Some(LoroValue::from("online")));

    sleep(Duration::from_millis(15));
    assert!(store.encode("status").is_empty());
    let encoded_all = store.encode_all();
    let imported = EphemeralStore::new(30_000);
    imported.apply(&encoded_all).unwrap();
    assert_eq!(imported.get("status"), None);

    store.remove_outdated();
    assert_eq!(store.get("status"), None);
    assert!(store.get_all_states().is_empty());
    assert_eq!(
        events.lock().unwrap().last(),
        Some(&(
            EphemeralEventTrigger::Timeout,
            vec![],
            vec![],
            vec!["status".to_string()],
        ))
    );
}

#[allow(deprecated)]
#[test]
fn legacy_awareness_selected_sync_stale_updates_and_timeout_follow_contract() {
    let mut alice = Awareness::new(1, 30_000);
    let mut bob = Awareness::new(2, 30_000);

    assert_eq!(alice.peer(), 1);
    assert_eq!(alice.get_local_state(), None);
    alice.set_local_state("typing");
    assert_eq!(alice.get_local_state(), Some(LoroValue::from("typing")));

    let selected = alice.encode(&[1]);
    let (updated, added) = bob.apply(&selected);
    assert!(updated.is_empty());
    assert_eq!(added, vec![1]);
    assert_eq!(
        bob.get_all_states().get(&1).map(|info| info.state.clone()),
        Some(LoroValue::from("typing"))
    );

    let stale = selected.clone();
    alice.set_local_state("idle");
    let all = alice.encode_all();
    let (updated, added) = bob.apply(&all);
    assert_eq!(updated, vec![1]);
    assert!(added.is_empty());
    assert_eq!(
        bob.get_all_states().get(&1).map(|info| info.state.clone()),
        Some(LoroValue::from("idle"))
    );

    let (updated, added) = bob.apply(&stale);
    assert!(updated.is_empty());
    assert!(added.is_empty());
    assert_eq!(
        bob.get_all_states().get(&1).map(|info| info.state.clone()),
        Some(LoroValue::from("idle"))
    );

    let empty_selection = alice.encode(&[42]);
    let (updated, added) = bob.apply(&empty_selection);
    assert!(updated.is_empty());
    assert!(added.is_empty());
}

#[test]
fn ephemeral_store_rejects_invalid_payloads_and_unsubscribes_false_callbacks() {
    let store = EphemeralStore::new(30_000);

    let local_payloads = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let payloads_clone = Arc::clone(&local_payloads);
    let _local_updates = store.subscribe_local_updates(Box::new(move |bytes| {
        payloads_clone.lock().unwrap().push(bytes.clone());
        false
    }));

    store.set("cursor", 1);
    store.set("cursor", 2);
    assert_eq!(local_payloads.lock().unwrap().len(), 1);
    assert!(store.apply(&[0xff, 0xff, 0xff, 0xff]).is_err());

    let events = Arc::new(Mutex::new(0));
    let events_clone = Arc::clone(&events);
    let _sub = store.subscribe(Box::new(move |_| {
        *events_clone.lock().unwrap() += 1;
        false
    }));
    store.delete("cursor");
    store.set("cursor", 3);
    assert_eq!(*events.lock().unwrap(), 1);
}
