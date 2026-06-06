//! Encoding and decoding of mergeable container ids.

use loro_internal::ContainerType;

#[test]
#[cfg(feature = "counter")]
fn mergeable_container_id_roundtrips_parent_key_and_type() {
    let parent = loro_common::ContainerID::new_root("state", ContainerType::Map);
    let key = "field\u{1}with/slash:and:semicolon";
    let cid = loro_common::ContainerID::new_mergeable(&parent, key, ContainerType::Counter);

    assert!(cid.is_mergeable());
    let (decoded_parent, decoded_key, decoded_type) = cid.parse_mergeable().unwrap();
    assert_eq!(decoded_parent, parent);
    assert_eq!(decoded_key, key);
    assert_eq!(decoded_type, ContainerType::Counter);
}

#[test]
fn user_root_names_cannot_use_mergeable_namespace() {
    assert!(!loro_common::check_root_container_name(
        loro_common::MERGEABLE_NAMESPACE_PREFIX
    ));
}

/// `parse_mergeable` is a pure decoder and must return `None` (not panic, not
/// silently misinterpret) for every malformed payload. This guards against
/// future drift in the encoder + decoder pair.
#[test]
#[cfg(feature = "counter")]
fn parse_mergeable_rejects_malformed_payloads() {
    use loro_common::{ContainerID, ContainerType};

    // Non-mergeable root: returns None.
    let plain_root = ContainerID::new_root("ordinary", ContainerType::Map);
    assert!(plain_root.parse_mergeable().is_none());

    // Mergeable prefix but invalid hex.
    let bad_hex = ContainerID::Root {
        name: "🤝:zzzz".into(),
        container_type: ContainerType::Counter,
    };
    assert!(
        bad_hex.parse_mergeable().is_none(),
        "non-hex chars in payload must reject"
    );

    // Mergeable prefix, valid hex, but truncated (no segments).
    let truncated = ContainerID::Root {
        name: "🤝:".into(),
        container_type: ContainerType::Counter,
    };
    assert!(
        truncated.parse_mergeable().is_none(),
        "empty payload must reject"
    );

    // Mergeable prefix, valid hex, but trailing garbage after the type byte.
    let parent = ContainerID::new_root("state", ContainerType::Map);
    let cid = ContainerID::new_mergeable(&parent, "k", ContainerType::Counter);
    let mut name = match &cid {
        ContainerID::Root { name, .. } => name.to_string(),
        _ => panic!("expected Root"),
    };
    name.push_str("ff"); // append one extra byte's worth of hex
    let with_garbage = ContainerID::Root {
        name: name.into(),
        container_type: ContainerType::Counter,
    };
    assert!(
        with_garbage.parse_mergeable().is_none(),
        "trailing bytes after type byte must reject"
    );

    // Mergeable prefix and a payload that decodes correctly, BUT the
    // encoded type byte disagrees with the Root's container_type field.
    let mismatched = ContainerID::Root {
        name: match &cid {
            ContainerID::Root { name, .. } => name.clone(),
            _ => unreachable!(),
        },
        container_type: ContainerType::Map, // payload says Counter
    };
    assert!(
        mismatched.parse_mergeable().is_none(),
        "type-byte mismatch with Root.container_type must reject"
    );
}

/// `LoroDoc::get_map` must reject names in the mergeable namespace at the
/// call site, not just in `check_root_container_name`. Otherwise user code
/// could fabricate a Root cid that masquerades as a mergeable child and
/// confuse the parent-edge walks.
///
/// This test runs `check_root_container_name` directly on a variety of
/// user-supplied strings; the runtime `get_map` / `get_text` / etc. calls
/// route through this validator (see callers in `crates/loro-internal/src/`).
/// If the validator is bypassed by a runtime path, that's a separate bug —
/// but it's not something this test can prove without intentionally writing
/// `🤝:` keys, which is what we're trying to prevent in the first place.
#[test]
fn root_name_validator_rejects_mergeable_namespace_inputs() {
    use loro_common::{check_root_container_name, MERGEABLE_NAMESPACE_PREFIX};

    // Bare prefix.
    assert!(!check_root_container_name(MERGEABLE_NAMESPACE_PREFIX));
    // Prefix + arbitrary payload.
    assert!(!check_root_container_name("🤝:deadbeef"));
    assert!(!check_root_container_name("🤝:"));
    // Prefix-as-substring is OK (not a prefix), validator still allows it.
    assert!(check_root_container_name("foo🤝:bar"));
    // Prefix with a leading zero-width space is NOT a prefix match, allowed.
    assert!(check_root_container_name("\u{200B}🤝:abc"));
    // Sanity: ordinary user names still pass.
    assert!(check_root_container_name("state"));
    assert!(check_root_container_name("ordinary-name_with-symbols"));
    // Empty is still rejected (pre-existing behavior).
    assert!(!check_root_container_name(""));
    // Slash and NUL still rejected (pre-existing behavior).
    assert!(!check_root_container_name("a/b"));
    assert!(!check_root_container_name("a\0b"));
}

/// For each supported container kind, `new_mergeable` produces a deterministic
/// cid that decodes back to the same `(parent, key, kind)`. Counter is gated
/// on the feature; the rest are unconditional.
#[test]
fn mergeable_cid_roundtrips_for_every_container_kind() {
    use loro_common::{ContainerID, ContainerType};
    let parent = ContainerID::new_root("state", ContainerType::Map);

    let mut kinds: Vec<ContainerType> = vec![
        ContainerType::Map,
        ContainerType::List,
        ContainerType::MovableList,
        ContainerType::Text,
        ContainerType::Tree,
    ];
    #[cfg(feature = "counter")]
    kinds.push(ContainerType::Counter);

    for kind in kinds {
        let cid = ContainerID::new_mergeable(&parent, "field", kind);
        assert!(cid.is_mergeable(), "kind {kind:?}: must be mergeable");
        let again = ContainerID::new_mergeable(&parent, "field", kind);
        assert_eq!(cid, again, "kind {kind:?}: cid must be deterministic");
        let (decoded_parent, decoded_key, decoded_kind) = cid
            .parse_mergeable()
            .unwrap_or_else(|| panic!("kind {kind:?}: parse_mergeable returned None"));
        assert_eq!(decoded_parent, parent, "kind {kind:?}: parent roundtrip");
        assert_eq!(decoded_key, "field", "kind {kind:?}: key roundtrip");
        assert_eq!(decoded_kind, kind, "kind {kind:?}: kind roundtrip");
    }
}

/// Key encoding is len-prefixed binary, so it must round-trip cleanly for
/// degenerate inputs: empty, long, embedded NUL, and embedded mergeable
/// prefix substring. Catches off-by-one and ad-hoc string-split mistakes
/// in any future decoder change.
#[test]
fn mergeable_cid_roundtrips_for_degenerate_keys() {
    use loro_common::{ContainerID, ContainerType};
    let parent = ContainerID::new_root("state", ContainerType::Map);

    let long_key: String = std::iter::repeat('k').take(2048).collect();
    let cases: Vec<&str> = vec![
        "",
        long_key.as_str(),
        "with\0nul\0bytes",
        "embedded 🤝: substring in the middle",
        "starts_with_🤝:_prefix",
        "trailing_emoji_🤝:",
        "ascii/slash/looking",
    ];

    for key in cases {
        let cid = ContainerID::new_mergeable(&parent, key, ContainerType::Map);
        assert!(cid.is_mergeable(), "key {key:?}: must be mergeable");
        let (decoded_parent, decoded_key, decoded_kind) = cid
            .parse_mergeable()
            .unwrap_or_else(|| panic!("key {key:?}: parse_mergeable returned None"));
        assert_eq!(decoded_parent, parent);
        assert_eq!(decoded_key, key, "key {key:?}: round-trip mismatch");
        assert_eq!(decoded_kind, ContainerType::Map);
    }
}
