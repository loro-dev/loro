use loro::{cursor::PosType, ExpandType, LoroDoc, StyleConfig, StyleConfigMap, TextDelta};
use std::{hint::black_box, time::Instant};

#[test]
fn slice_end_excludes_adjacent_zero_width_style_anchors() {
    for expand in [ExpandType::After, ExpandType::None] {
        let doc = LoroDoc::new();
        let mut config = StyleConfigMap::new();
        config.insert("style".into(), StyleConfig { expand });
        doc.config_text_style(config);
        let text = doc.get_text("text");
        for i in 0..100 {
            text.insert(i * 3, "a😀文").unwrap();
            text.mark(i * 3..(i + 1) * 3, "style", i as i32).unwrap();
        }
        doc.commit();
        for i in 0..100 {
            assert_eq!(text.slice(i * 3, (i + 1) * 3).unwrap(), "a😀文");
            let expected = vec![TextDelta::Insert {
                insert: "a😀文".into(),
                attributes: Some([("style".into(), (i as i32).into())].into_iter().collect()),
            }];
            for (kind, width) in [
                (PosType::Unicode, 3),
                (PosType::Utf16, 4),
                (PosType::Bytes, 8),
            ] {
                assert_eq!(
                    text.slice_delta(i * width, (i + 1) * width, kind).unwrap(),
                    expected
                );
            }
        }
        assert!(text.slice_delta(1, 2, PosType::Utf16).is_err());
        assert!(text.slice_delta(1, 2, PosType::Bytes).is_err());
    }
}

// Run in release mode with --ignored --nocapture. Compare a constant-sized
// range at the beginning and end; construction and commits are not timed.
#[test]
#[ignore]
fn slices_scale_with_range_not_prefix() {
    let repetitions = std::env::var("LORO_SLICE_REPETITIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000usize);
    for n in [1000usize, 2000, 4000, 8000] {
        let doc = LoroDoc::new();
        let text = doc.get_text("text");
        let block = "a😀bcdefghij";
        let width = block.chars().count();
        for i in 0..n {
            text.insert(i * width, block).unwrap();
            text.mark(i * width..(i + 1) * width, "bold", i % 2 == 0)
                .unwrap();
        }
        doc.commit();
        for pos in [0, (n - 1) * width] {
            for delta in [false, true] {
                let start = Instant::now();
                for _ in 0..repetitions {
                    if delta {
                        black_box(
                            text.slice_delta(pos, pos + width, PosType::Unicode)
                                .unwrap(),
                        );
                    } else {
                        black_box(text.slice(pos, pos + width).unwrap());
                    }
                }
                println!(
                    "blocks={n} pos={pos} delta={delta} ns/call={:.1}",
                    start.elapsed().as_nanos() as f64 / repetitions as f64
                );
            }
        }
    }
}
