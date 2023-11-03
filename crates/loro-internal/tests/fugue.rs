use loro_common::LoroResult;
use loro_internal::{LoroDoc, ToJson};
use serde_json::json;

#[test]
fn test_forward_interleaving() -> LoroResult<()> {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(0)?;
    a.get_text("text").insert_(0, "Hello")?;
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(1)?;
    b.get_text("text").insert_(0, " World!")?;
    a.merge(&b)?;
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"text": "Hello World!"})
    );
    Ok(())
}

#[test]
fn test_backward_interleaving() -> LoroResult<()> {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(0)?;
    a.get_text("text").insert_(0, "o")?;
    a.get_text("text").insert_(0, "l")?;
    a.get_text("text").insert_(0, "l")?;
    a.get_text("text").insert_(0, "e")?;
    a.get_text("text").insert_(0, "H")?;
    dbg!(a.get_deep_value());
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(1)?;
    b.get_text("text").insert_(0, "!")?;
    b.get_text("text").insert_(0, "d")?;
    b.get_text("text").insert_(0, "l")?;
    b.get_text("text").insert_(0, "r")?;
    b.get_text("text").insert_(0, "o")?;
    b.get_text("text").insert_(0, "W")?;
    b.get_text("text").insert_(0, " ")?;
    dbg!(b.get_deep_value());
    a.merge(&b)?;
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"text": "Hello World!"})
    );
    Ok(())
}

#[test]
fn test_forward_backward() -> LoroResult<()> {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(0)?;
    a.get_text("text").insert_(0, "ll")?;
    a.get_text("text").insert_(0, "He")?;
    a.get_text("text").insert_(4, "o")?;
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(1)?;
    b.get_text("text").insert_(0, " !")?;
    b.get_text("text").insert_(1, "W")?;
    b.get_text("text").insert_(2, "d")?;
    b.get_text("text").insert_(2, "l")?;
    b.get_text("text").insert_(2, "r")?;
    b.get_text("text").insert_(2, "o")?;
    a.merge(&b)?;
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"text": "Hello World!"})
    );

    Ok(())
}

#[test]
fn test_yjs_interleave() -> LoroResult<()> {
    // As stated in the Fugue paper, Yjs has a interleaving anomaly in the following case:
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(0)?;
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(1)?;
    let c = LoroDoc::new_auto_commit();
    c.set_peer_id(2)?;
    c.get_text("text").insert_(0, "2")?;
    a.merge(&c)?;
    a.get_text("text").insert_(0, "1")?;
    // b should not be between a and c
    b.get_text("text").insert_(0, "b")?;
    a.merge(&b)?;
    assert_eq!(a.get_deep_value().to_json_value(), json!({"text": "b12"}));
    Ok(())
}
