//! Temporary memory probe for loro#1092: measures live Rust heap bytes retained
//! by the container-by-container handle walk vs the bulk deep-value path.
//! Run: cargo test -p loro-internal --test mem_probe -- --nocapture

use dev_utils::get_mem_usage;
use loro_internal::handler::{Handler, ValueOrHandler};
use loro_internal::cursor::PosType;
use loro_internal::LoroDoc;

fn live_mib() -> f64 {
    get_mem_usage().0 as f64 / 2f64.powi(20)
}

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
                item.insert("toolCallId", format!("tc-{t}-{i}").as_str()).unwrap();
                let raw = item
                    .insert_container("rawInput", loro_internal::handler::MapHandler::new_detached())
                    .unwrap();
                raw.insert("command", "pnpm test").unwrap();
                raw.insert("cwd", "/repo").unwrap();
                let text = item
                    .insert_container("title", loro_internal::handler::TextHandler::new_detached())
                    .unwrap();
                text.insert(0, TEXT, PosType::Unicode).unwrap();
            }
        }
    }
    doc.export(loro_internal::encoding::ExportMode::snapshot()).unwrap()
}

fn walk(h: &Handler, handles: &mut usize) {
    *handles += 1;
    match h {
        Handler::Map(m) => {
            let keys: Vec<_> = m.keys().collect();
            for k in keys {
                if let Some(v) = m.get_(&k) {
                    if let ValueOrHandler::Handler(child) = v {
                        walk(&child, handles);
                    }
                }
            }
        }
        Handler::List(l) => {
            for i in 0..l.len() {
                if let Some(v) = l.get_(i) {
                    if let ValueOrHandler::Handler(child) = v {
                        walk(&child, handles);
                    }
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
fn probe_handle_walk_memory() {
    let snapshot = build_snapshot(95, 100); // ≈ 63k containers like the 190-turn repro
    println!("snapshot: {:.1} MiB", snapshot.len() as f64 / 2f64.powi(20));

    // Bulk path baseline
    let doc = LoroDoc::new_auto_commit();
    let base = live_mib();
    doc.import(&snapshot).unwrap();
    let after_import = live_mib();
    let _ = doc.get_deep_value();
    let after_deep = live_mib();
    println!("bulk:   import={after_import:.1} deep_value={after_deep:.1} MiB (base {base:.1})");
    drop(doc);
    let after_drop = live_mib();
    println!("bulk:   after doc drop: {after_drop:.1} MiB");

    // Handle path
    let doc = LoroDoc::new_auto_commit();
    doc.import(&snapshot).unwrap();
    let after_import2 = live_mib();
    let mut handles = 0;
    let history = Handler::List(doc.get_list("history"));
    walk(&history, &mut handles);
    let after_walk1 = live_mib();
    println!("handle: import={after_import2:.1} walk#1={after_walk1:.1} MiB handles={handles}");
    walk(&history, &mut handles);
    let after_walk2 = live_mib();
    println!("handle: walk#2={after_walk2:.1} MiB handles={handles}");
    drop(history);
    drop(doc);
    println!("handle: after doc drop: {:.1} MiB", live_mib());
}
