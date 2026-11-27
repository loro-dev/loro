//! Reads from an imported text that has not been decoded yet must match reads
//! from the same text once it is decoded.

use loro::{ExportMode, LoroDoc};

const TEXT: &str = "a😀b\u{e9}e\u{301}字😀😀z";

fn lazy_copy() -> (LoroDoc, LoroDoc) {
    let source = LoroDoc::new();
    source.get_text("t").insert(0, TEXT).unwrap();
    source.commit();
    let lazy = LoroDoc::new();
    lazy.import(&source.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    (source, lazy)
}

#[test]
fn lazy_char_at_matches_decoded() {
    let (source, lazy) = lazy_copy();
    let (a, b) = (source.get_text("t"), lazy.get_text("t"));
    for pos in 0..TEXT.chars().count() + 3 {
        assert_eq!(
            format!("{:?}", a.char_at(pos)),
            format!("{:?}", b.char_at(pos)),
            "char_at({pos})"
        );
    }
}

#[test]
fn lazy_slice_matches_decoded() {
    let (source, lazy) = lazy_copy();
    let (a, b) = (source.get_text("t"), lazy.get_text("t"));
    let unicode = TEXT.chars().count();
    let utf16 = TEXT.encode_utf16().count();
    for start in 0..unicode + 2 {
        for end in 0..unicode + 2 {
            assert_eq!(
                format!("{:?}", a.slice(start, end)),
                format!("{:?}", b.slice(start, end)),
                "slice({start}, {end})"
            );
        }
    }
    for start in 0..utf16 + 2 {
        for end in 0..utf16 + 2 {
            let (x, y) = (a.slice_utf16(start, end), b.slice_utf16(start, end));
            // Both must agree on success; error positions may differ in detail text only.
            assert_eq!(x.is_ok(), y.is_ok(), "slice_utf16({start}, {end})");
            if let (Ok(x), Ok(y)) = (x, y) {
                assert_eq!(x, y, "slice_utf16({start}, {end})");
            }
        }
    }
}

#[test]
fn iter_yields_whole_text_and_allows_edits_in_the_callback() {
    let (_, lazy) = lazy_copy();
    let text = lazy.get_text("t");
    let mut out = String::new();
    text.iter(|chunk| {
        out.push_str(chunk);
        true
    });
    assert_eq!(out, TEXT);

    let mut calls = 0;
    text.iter(|_| {
        calls += 1;
        text.insert(0, "!").unwrap();
        false
    });
    assert_eq!(calls, 1);
    assert_eq!(text.to_string(), format!("!{TEXT}"));
}
