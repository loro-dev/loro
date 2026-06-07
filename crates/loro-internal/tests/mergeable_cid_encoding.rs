//! Encoding and decoding of mergeable container ids.

#[test]
#[cfg(feature = "counter")]
fn mergeable_container_id_roundtrips_parent_key_and_type() {
    use loro_internal::ContainerType;

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
///
/// The cid does NOT encode the container kind in its name — kind already lives in
/// `ContainerID::Root::container_type`. Distinct kinds at the same `(parent, key)` therefore share
/// the same name string but compare unequal at the `ContainerID` level, which is what callers see.
#[test]
#[cfg(feature = "counter")]
fn parse_mergeable_rejects_malformed_payloads() {
    use loro_common::{ContainerID, ContainerType};

    // Non-mergeable root: returns None.
    let plain_root = ContainerID::new_root("ordinary", ContainerType::Map);
    assert!(plain_root.parse_mergeable().is_none());

    // Mergeable prefix but missing the required key segment.
    let no_key = ContainerID::Root {
        name: "🤝:$state".into(),
        container_type: ContainerType::Counter,
    };
    assert!(
        no_key.parse_mergeable().is_none(),
        "payload without a key segment must reject"
    );

    // Mergeable prefix, but empty payload.
    let truncated = ContainerID::Root {
        name: "🤝:".into(),
        container_type: ContainerType::Counter,
    };
    assert!(
        truncated.parse_mergeable().is_none(),
        "empty payload must reject"
    );

    // Unknown escapes, raw slash, and dangling backslash must reject.
    let malformed = ContainerID::Root {
        name: "🤝:$state>bad\\xescape".into(),
        container_type: ContainerType::Counter,
    };
    assert!(
        malformed.parse_mergeable().is_none(),
        "unknown escape in payload must reject"
    );

    let raw_slash = ContainerID::Root {
        name: "🤝:$state>raw/slash".into(),
        container_type: ContainerType::Counter,
    };
    assert!(raw_slash.parse_mergeable().is_none());

    let dangling = ContainerID::Root {
        name: "🤝:$state>dangling\\".into(),
        container_type: ContainerType::Counter,
    };
    assert!(dangling.parse_mergeable().is_none());

    for name in [
        "🤝:@Z:0>key",
        "🤝:@001:0>key",
        "🤝:@1:00>key",
        "🤝:@1:-0>key",
    ] {
        let non_canonical = ContainerID::Root {
            name: name.into(),
            container_type: ContainerType::Counter,
        };
        assert!(
            non_canonical.parse_mergeable().is_none(),
            "non-canonical base36 payload must reject: {name:?}"
        );
    }
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

    for kind in ContainerType::ALL_TYPES {
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

/// Key encoding is escaped path segments, so it must round-trip cleanly for
/// degenerate inputs: empty, long, delimiters, embedded NUL, and embedded
/// mergeable prefix substring. Catches off-by-one and ad-hoc string-split
/// mistakes in any future decoder change.
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
        "contains>separator",
        "contains\\backslash",
        "mixed>\\/\0chars",
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

#[test]
fn mergeable_cid_uses_flattened_path_payload() {
    use loro_common::{ContainerID, ContainerType, MERGEABLE_NAMESPACE_PREFIX};

    let parent = ContainerID::new_root("state", ContainerType::Map);
    let child = ContainerID::new_mergeable(&parent, "note>1", ContainerType::Map);
    let grandchild = ContainerID::new_mergeable(&child, "body/slash\\nul\0", ContainerType::Text);

    let name = match &grandchild {
        ContainerID::Root { name, .. } => name.as_str(),
        _ => panic!("expected Root"),
    };
    assert!(
        name.starts_with(MERGEABLE_NAMESPACE_PREFIX),
        "mergeable cid must use the reserved namespace"
    );
    assert_eq!(
        name, "🤝:$state>note\\>1>body\\sslash\\\\nul\\0",
        "nested mergeable ids must extend the flattened path, not embed parent cid bytes"
    );
    assert!(
        !name.contains('/'),
        "synthetic root names must not contain raw slash"
    );
    assert!(
        !name.contains('\0'),
        "synthetic root names must not contain raw NUL"
    );

    let (decoded_parent, decoded_key, decoded_kind) = grandchild.parse_mergeable().unwrap();
    assert_eq!(decoded_parent, child);
    assert_eq!(decoded_key, "body/slash\\nul\0");
    assert_eq!(decoded_kind, ContainerType::Text);
}

#[test]
fn mergeable_cid_roundtrips_normal_base_parent() {
    use loro_common::{ContainerID, ContainerType, ID};

    let parent = ContainerID::new_normal(
        ID {
            peer: u64::MAX,
            counter: i32::MIN,
        },
        ContainerType::Map,
    );
    let cid = ContainerID::new_mergeable(&parent, "field", ContainerType::List);
    let name = match &cid {
        ContainerID::Root { name, .. } => name.as_str(),
        _ => panic!("expected Root"),
    };
    assert!(
        name.starts_with("🤝:@3w5e11264sgsf:-zik0zk>"),
        "normal parent ids should use compact base36 encoding; got {name:?}"
    );

    let (decoded_parent, decoded_key, decoded_kind) = cid.parse_mergeable().unwrap();
    assert_eq!(decoded_parent, parent);
    assert_eq!(decoded_key, "field");
    assert_eq!(decoded_kind, ContainerType::List);
}

#[test]
fn mergeable_cid_roundtrips_escaped_root_base_parent() {
    use loro_common::{ContainerID, ContainerType};

    let parent = ContainerID::new_root("root>\\name", ContainerType::Map);
    let cid = ContainerID::new_mergeable(&parent, "field", ContainerType::Tree);
    let name = match &cid {
        ContainerID::Root { name, .. } => name.as_str(),
        _ => panic!("expected Root"),
    };
    assert_eq!(name, "🤝:$root\\>\\\\name>field");

    let (decoded_parent, decoded_key, decoded_kind) = cid.parse_mergeable().unwrap();
    assert_eq!(decoded_parent, parent);
    assert_eq!(decoded_key, "field");
    assert_eq!(decoded_kind, ContainerType::Tree);
}

#[test]
#[should_panic(expected = "mergeable child parent must be a map")]
fn mergeable_cid_rejects_non_map_parent() {
    use loro_common::{ContainerID, ContainerType};

    let parent = ContainerID::new_root("content", ContainerType::Text);
    let _ = ContainerID::new_mergeable(&parent, "field", ContainerType::Map);
}

#[test]
fn nested_mergeable_cid_size_grows_linearly() {
    use loro_common::{ContainerID, ContainerType};

    let mut cid = ContainerID::new_root("state", ContainerType::Map);
    let mut sizes = Vec::new();
    for _ in 0..=8 {
        cid = ContainerID::new_mergeable(&cid, "k", ContainerType::Map);
        sizes.push(cid.to_bytes().len());
    }

    for window in sizes.windows(2) {
        assert_eq!(
            window[1] - window[0],
            2,
            "each additional one-byte key should add only separator + key bytes; sizes={sizes:?}"
        );
    }
    assert!(
        sizes[8] < 64,
        "depth 8 should stay compact with flattened encoding; sizes={sizes:?}"
    );
}
