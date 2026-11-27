//! A diff between versions with concurrent marks must give the target's styles.

use loro::{ExportMode, LoroDoc, LoroResult, ToJson};
use serde_json::json;

fn peer(id: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(id).unwrap();
    doc
}

fn send(from: &LoroDoc, to: &LoroDoc) -> LoroResult<()> {
    from.commit();
    to.import(&from.export(ExportMode::all_updates())?)?;
    Ok(())
}

#[test]
fn diff_between_concurrent_marks_matches_target_richtext() -> LoroResult<()> {
    let a = peer(1);
    let b = peer(2);
    a.get_text("text").insert(0, "abc")?;
    send(&a, &b)?;

    a.get_text("text").mark(0..3, "bold", true)?;
    a.commit();
    let from = a.state_frontiers();
    let source_snapshot = a.export(ExportMode::Snapshot)?;

    b.get_text("text").mark(1..2, "italic", true)?;
    b.commit();
    let to = b.state_frontiers();
    let expected = b.get_text("text").get_richtext_value();
    assert_eq!(
        expected.to_json_value(),
        json!([
            {"insert": "a"},
            {"insert": "b", "attributes": {"italic": true}},
            {"insert": "c"}
        ])
    );

    send(&b, &a)?;
    let patch = a.diff(&from, &to)?;
    a.checkout(&to)?;
    assert_eq!(a.get_text("text").get_richtext_value(), expected);

    let patched = LoroDoc::from_snapshot(&source_snapshot)?;
    patched.apply_diff(patch)?;
    assert_eq!(patched.get_text("text").get_richtext_value(), expected);
    Ok(())
}

/// Checkout events must carry the same styles, so an event mirror stays right.
#[test]
fn checkout_event_between_concurrent_marks_matches_target_richtext() -> LoroResult<()> {
    use std::sync::{Arc, Mutex};
    let a = peer(1);
    let b = peer(2);
    a.get_text("text").insert(0, "abc")?;
    send(&a, &b)?;
    a.get_text("text").mark(0..3, "bold", true)?;
    a.commit();
    let from = a.state_frontiers();
    b.get_text("text").mark(1..2, "italic", true)?;
    b.commit();
    let to = b.state_frontiers();
    let expected = b.get_text("text").get_richtext_value();
    send(&b, &a)?;

    a.checkout(&from)?;
    let mirror = LoroDoc::from_snapshot(&a.export(ExportMode::state_only(Some(&from)))?)?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    let _sub = a.subscribe_root(Arc::new(move |e| {
        for c in e.events {
            if let loro::event::Diff::Text(d) = &c.diff {
                seen2.lock().unwrap().push(d.clone());
            }
        }
    }));
    a.checkout(&to)?;
    let text = mirror.get_text("text");
    for d in seen.lock().unwrap().iter() {
        text.apply_delta(d)?;
    }
    assert_eq!(text.get_richtext_value(), expected);
    Ok(())
}
