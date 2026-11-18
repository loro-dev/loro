use loro::{ExportMode, LoroDoc};

/// One peer types "Hello world" and makes "Hello" bold: three ops of lengths
/// 11, 1 and 1. The peer id is fixed, so the bytes are the same on every run.
fn valid_export() -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "Hello world").unwrap();
    text.mark(0..5, "bold", true).unwrap();
    doc.commit();
    doc.export(ExportMode::all_updates()).unwrap()
}

/// Changes one body byte and rewrites the header checksum to match.
fn damage(mut bytes: Vec<u8>, offset: usize, from: u8, to: u8) -> Vec<u8> {
    assert_eq!(bytes[offset], from, "the export format changed");
    bytes[offset] = to;
    let sum = xxhash_rust::xxh32::xxh32(&bytes[20..], u32::from_le_bytes(*b"LORO"));
    bytes[16..20].copy_from_slice(&sum.to_le_bytes());
    bytes
}

/// loro-dev/loro#1293: a text insert whose length column disagrees with the
/// characters it carries must be refused, not panic or be taken in.
#[test]
fn text_insert_length_must_match_its_text() {
    let valid = valid_export();
    // Lengths 11, 1, 1 become 4, 4, 1: the import panicked in the diff calculator.
    // Lengths 11, 1, 1 become 3, 1, 1: the import succeeded and the doc could
    // not export a readable copy of itself.
    for (offset, from, to) in [(85, 0x01, 0x80), (86, 0x0b, 0x03)] {
        let bytes = damage(valid.clone(), offset, from, to);
        let doc = LoroDoc::new();
        assert!(doc.import(&bytes).is_err(), "byte {offset}");
        assert_eq!(doc.get_text("text").to_string(), "");
        assert!(doc.oplog_vv().is_empty());
        // The doc is still usable.
        doc.import(&valid).unwrap();
        assert_eq!(doc.get_text("text").to_string(), "Hello world");
    }
}
