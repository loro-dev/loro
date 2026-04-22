use loro::{ExportMode, LoroDoc, LoroResult};
use pretty_assertions::assert_eq;

#[test]
fn importing_many_sparse_text_edits_preserves_plain_text_state() -> LoroResult<()> {
    let alice = LoroDoc::new();
    alice.set_peer_id(131)?;
    let text = alice.get_text("text");
    let mut expected = "a".repeat(900);
    text.insert(0, &expected)?;
    alice.commit();

    let bob = LoroDoc::from_snapshot(&alice.export(ExportMode::Snapshot)?)?;
    let bob_vv = bob.oplog_vv();

    for i in 0..300 {
        let pos = i * 3;
        text.delete(pos, 1)?;
        text.insert(pos, "b")?;
        expected.replace_range(pos..pos + 1, "b");
    }
    alice.commit();

    let updates = alice.export(ExportMode::updates(&bob_vv))?;
    bob.import(&updates)?;

    assert_eq!(bob.get_text("text").to_string(), expected);

    Ok(())
}
