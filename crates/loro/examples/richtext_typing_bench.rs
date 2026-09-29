//! Local typing cost in a text with style anchors (loro-dev/loro#1135 review).
//!
//! ```sh
//! cargo run -p loro --release --example richtext_typing_bench -- in-bold commit
//! ```
//!
//! Each run builds a 100k-character text, applies the scenario's marks, then
//! times 100k single-character inserts at consecutive positions, committing
//! after every insert (`commit`) or once at the end (`txn`). Scenarios:
//!
//! - `plain`: no style.
//! - `in-bold`: in the middle of a bold range.
//! - `bold-end`: at the end of a bold range (the text goes before the end
//!   anchor, so each insert runs the anchor rules).
//! - `marks-2000`: between two of 2,000 short bold ranges.
//!
//! Prints `scenario,mode,ms`.

use loro::{ExpandType, LoroDoc, StyleConfig, StyleConfigMap};
use std::time::Instant;

const LEN: usize = 100_000;
const INPUTS: usize = 100_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let scenario = args.next().expect("scenario");
    let mode = args.next().unwrap_or_else(|| "commit".into());
    let commit_each = match mode.as_str() {
        "commit" => true,
        "txn" => false,
        _ => panic!("mode must be commit or txn"),
    };

    let doc = LoroDoc::new();
    let mut styles = StyleConfigMap::new();
    styles.insert("bold".into(), StyleConfig::new().expand(ExpandType::After));
    doc.config_text_style(styles);
    let text = doc.get_text("text");
    text.insert(0, &"a".repeat(LEN)).unwrap();
    let start = match scenario.as_str() {
        "plain" => LEN / 2,
        "in-bold" => {
            text.mark(0..LEN, "bold", true).unwrap();
            LEN / 2
        }
        "bold-end" => {
            text.mark(0..LEN / 2, "bold", true).unwrap();
            LEN / 2
        }
        "marks-2000" => {
            for i in 0..2000 {
                text.mark(i * 50..i * 50 + 10, "bold", true).unwrap();
            }
            1000 * 50 + 30
        }
        _ => panic!("unknown scenario {scenario}"),
    };
    doc.commit();

    let t = Instant::now();
    for i in 0..INPUTS {
        text.insert(start + i, "x").unwrap();
        if commit_each {
            doc.commit();
        }
    }
    doc.commit();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(text.len_unicode(), LEN + INPUTS);
    println!("{scenario},{mode},{ms:.1}");
}
