//! Regression test for loro#1092: a container-by-container handle walk must
//! retain bounded memory. The decoded-value cache in `InnerStore` is capped
//! (`MAX_CACHED_CONTAINER_VALUES`), so retained memory after a walk is
//! O(cache bound), not O(containers ever read).
//!
//! Run: cargo test -p loro-internal --test handle_walk_memory -- --nocapture

use dev_utils::get_mem_usage;
use loro_internal::cursor::PosType;
use loro_internal::handler::{Handler, ValueOrHandler};
use loro_internal::LoroDoc;

fn live_bytes() -> usize {
    get_mem_usage().0
}

const MIB: usize = 1 << 20;

const TEXT: &str = "The quick brown fox jumps over the lazy dog. \
The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.";

fn build_snapshot(turns: usize, items_per_turn: usize) -> Vec<u8> {
    let doc = LoroDoc::new_auto_commit();
    let history = doc.get_list("history");
    for t in 0..turns {
        let turn = history
            .push_container(loro_internal::handler::MapHandler::new_detached())
            .unwrap();
        turn.insert("id", format!("turn-{t}").as_str()).unwrap();
        turn.insert("role", if t % 2 == 0 { "user" } else { "assistant" })
            .unwrap();
        let items = turn
            .insert_container("items", loro_internal::handler::ListHandler::new_detached())
            .unwrap();
        let count = if t % 2 == 0 { 1 } else { items_per_turn };
        for i in 0..count {
            let item = items
                .push_container(loro_internal::handler::MapHandler::new_detached())
                .unwrap();
            if i % 4 == 3 {
                item.insert("type", "text").unwrap();
                let text = item
                    .insert_container("text", loro_internal::handler::TextHandler::new_detached())
                    .unwrap();
                text.insert(0, &format!("{TEXT} turn {t} item {i}"), PosType::Unicode)
                    .unwrap();
            } else {
                item.insert("type", "tool_call").unwrap();
                item.insert("toolCallId", format!("tc-{t}-{i}").as_str())
                    .unwrap();
                let raw = item
                    .insert_container(
                        "rawInput",
                        loro_internal::handler::MapHandler::new_detached(),
                    )
                    .unwrap();
                raw.insert("command", "pnpm test").unwrap();
                raw.insert("cwd", "/repo").unwrap();
                let text = item
                    .insert_container(
                        "title",
                        loro_internal::handler::TextHandler::new_detached(),
                    )
                    .unwrap();
                text.insert(0, TEXT, PosType::Unicode).unwrap();
            }
        }
    }
    doc.export(loro_internal::encoding::ExportMode::snapshot())
        .unwrap()
}

/// The loro-mirror-style traversal: keys()/get() per map, get(i) per list,
/// to_string() per text.
fn walk(h: &Handler, handles: &mut usize) {
    *handles += 1;
    match h {
        Handler::Map(m) => {
            let keys: Vec<_> = m.keys().collect();
            for k in keys {
                if let Some(ValueOrHandler::Handler(child)) = m.get_(&k) {
                    walk(&child, handles);
                }
            }
        }
        Handler::List(l) => {
            for i in 0..l.len() {
                if let Some(ValueOrHandler::Handler(child)) = l.get_(i) {
                    walk(&child, handles);
                }
            }
        }
        Handler::Text(t) => {
            let _ = t.to_string();
        }
        _ => {}
    }
}

#[test]
fn handle_walk_retains_bounded_memory() {
    let snapshot = build_snapshot(95, 100); // ≈ 63k containers
    let doc = LoroDoc::new_auto_commit();
    doc.import(&snapshot).unwrap();
    let history = Handler::List(doc.get_list("history"));

    let after_import = live_bytes();
    let mut handles = 0;
    walk(&history, &mut handles);
    let after_walk1 = live_bytes();
    walk(&history, &mut handles);
    let after_walk2 = live_bytes();

    let walk1_retained = after_walk1.saturating_sub(after_import);
    let walk2_growth = after_walk2.saturating_sub(after_walk1);
    println!(
        "handles={handles} retained after walk#1: {:.1} MiB, walk#2 growth: {:.1} MiB",
        walk1_retained as f64 / MIB as f64,
        walk2_growth as f64 / MIB as f64,
    );

    // Unbounded, this walk pinned ~1 KB per container (~60 MB at this size).
    // The bounded cache (2048 entries) must retain far less than that.
    assert!(
        walk1_retained < 16 * MIB,
        "walk retained {:.1} MiB; the decoded-value cache is not bounded",
        walk1_retained as f64 / MIB as f64
    );
    // Re-walking hits the same bounded cache: no further growth.
    assert!(
        walk2_growth < 4 * MIB,
        "second walk grew memory by {:.1} MiB; cached values must be reused or re-decoded, not accumulated",
        walk2_growth as f64 / MIB as f64
    );
}
