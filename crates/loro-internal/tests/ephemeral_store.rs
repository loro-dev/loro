use loro_internal::awareness::EphemeralStore;
use loro_internal::LoroValue;
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
struct WireState {
    key: String,
    value: Option<LoroValue>,
    timestamp: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn import_skips_entries_past_timeout() {
    let store = EphemeralStore::new(100);
    let stale_timestamp = now_ms() - 500;

    let payload = postcard::to_allocvec(&vec![WireState {
        key: "stale".into(),
        value: Some(LoroValue::from(1)),
        timestamp: stale_timestamp,
    }])
    .unwrap();

    store.apply(&payload).unwrap();

    assert!(store.get("stale").is_none());
    assert!(store.get_all_states().is_empty());
}

#[test]
fn import_preserves_remote_timestamp_for_timeout() {
    let timeout_ms = 100;
    let store = EphemeralStore::new(timeout_ms);
    let remote_timestamp = now_ms() - (timeout_ms - 20);

    let payload = postcard::to_allocvec(&vec![WireState {
        key: "cursor".into(),
        value: Some(LoroValue::from("v")),
        timestamp: remote_timestamp,
    }])
    .unwrap();

    store.apply(&payload).unwrap();
    assert_eq!(store.get("cursor"), Some(LoroValue::from("v")));

    std::thread::sleep(Duration::from_millis(40));
    store.remove_outdated();

    assert!(store.get("cursor").is_none());
    assert!(store.get_all_states().is_empty());
}

#[test]
fn import_rejects_too_deep_loro_value() {
    let mut payload = vec![0x01u8, 0x00, 0x01];
    for _ in 0..600 {
        payload.push(0x05);
        payload.push(0x01);
    }
    payload.extend_from_slice(&[0x00, 0x00]);

    let store = EphemeralStore::new(30_000);
    let err = store.apply(&payload).unwrap_err();

    assert!(err.contains("Failed to decode data"));
}

#[test]
fn import_accepts_loro_value_at_depth_511() {
    let mut value = LoroValue::Null;
    for _ in 0..511 {
        value = LoroValue::List(vec![value].into());
    }

    let payload = postcard::to_allocvec(&vec![WireState {
        key: "deep".into(),
        value: Some(value),
        timestamp: now_ms(),
    }])
    .unwrap();

    let store = EphemeralStore::new(30_000);

    assert!(store.apply(&payload).is_ok());
    assert!(store.get("deep").is_some());
}
