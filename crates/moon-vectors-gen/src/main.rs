use std::fmt::Write as _;

use loro::{CommitOptions, ExportMode, LoroDoc, ToJson};

struct RustOracleVectorCase {
    name: &'static str,
    updates: Vec<Vec<u8>>,
    expected_json: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn escape_moonbit_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn doc_with_peer(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).expect("set_peer_id");
    doc
}

fn commit_fixed(doc: &LoroDoc) {
    // Persist a deterministic timestamp so exported updates are stable.
    doc.commit_with(CommitOptions::new().timestamp(0));
}

fn make_case_concurrent_list_insert_same_pos() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    a.get_list("list").insert(0, "A").unwrap();
    commit_fixed(&a);
    let updates_a = a.export(ExportMode::all_updates()).unwrap();

    let b = doc_with_peer(2);
    b.get_list("list").insert(0, "B").unwrap();
    commit_fixed(&b);
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_a).unwrap();
    oracle.import(&updates_b).unwrap();

    RustOracleVectorCase {
        name: "concurrent_list_insert_same_pos",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn make_case_concurrent_text_insert_same_pos() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    a.get_text("text").insert(0, "A").unwrap();
    commit_fixed(&a);
    let updates_a = a.export(ExportMode::all_updates()).unwrap();

    let b = doc_with_peer(2);
    b.get_text("text").insert(0, "B").unwrap();
    commit_fixed(&b);
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_a).unwrap();
    oracle.import(&updates_b).unwrap();

    RustOracleVectorCase {
        name: "concurrent_text_insert_same_pos",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn make_case_concurrent_list_insert_vs_delete() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    {
        let list = a.get_list("list");
        list.insert(0, "A").unwrap();
        list.insert(1, "B").unwrap();
    }
    commit_fixed(&a);

    let base = a.export(ExportMode::all_updates()).unwrap();
    let b = doc_with_peer(2);
    b.import(&base).unwrap();

    a.get_list("list").delete(1, 1).unwrap();
    commit_fixed(&a);

    b.get_list("list").insert(1, "X").unwrap();
    commit_fixed(&b);

    let updates_a = a.export(ExportMode::all_updates()).unwrap();
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_b).unwrap();
    oracle.import(&updates_a).unwrap();

    RustOracleVectorCase {
        name: "concurrent_list_insert_vs_delete",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn make_case_concurrent_text_insert_vs_delete() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    a.get_text("text").insert(0, "AB").unwrap();
    commit_fixed(&a);

    let base = a.export(ExportMode::all_updates()).unwrap();
    let b = doc_with_peer(2);
    b.import(&base).unwrap();

    a.get_text("text").delete(1, 1).unwrap();
    commit_fixed(&a);

    b.get_text("text").insert(1, "X").unwrap();
    commit_fixed(&b);

    let updates_a = a.export(ExportMode::all_updates()).unwrap();
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_a).unwrap();
    oracle.import(&updates_b).unwrap();

    RustOracleVectorCase {
        name: "concurrent_text_insert_vs_delete",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn make_case_concurrent_movable_list_insert_same_pos() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    a.get_movable_list("ml").insert(0, "A").unwrap();
    commit_fixed(&a);
    let updates_a = a.export(ExportMode::all_updates()).unwrap();

    let b = doc_with_peer(2);
    b.get_movable_list("ml").insert(0, "B").unwrap();
    commit_fixed(&b);
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_a).unwrap();
    oracle.import(&updates_b).unwrap();

    RustOracleVectorCase {
        name: "concurrent_movable_list_insert_same_pos",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn make_case_concurrent_movable_list_move_same_elem() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    {
        let ml = a.get_movable_list("ml");
        ml.insert(0, "a").unwrap();
        ml.insert(1, "b").unwrap();
        ml.insert(2, "c").unwrap();
    }
    commit_fixed(&a);

    let base = a.export(ExportMode::all_updates()).unwrap();
    let b = doc_with_peer(2);
    b.import(&base).unwrap();

    a.get_movable_list("ml").mov(0, 2).unwrap();
    commit_fixed(&a);

    b.get_movable_list("ml").mov(0, 1).unwrap();
    commit_fixed(&b);

    let updates_a = a.export(ExportMode::all_updates()).unwrap();
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_a).unwrap();
    oracle.import(&updates_b).unwrap();

    RustOracleVectorCase {
        name: "concurrent_movable_list_move_same_elem",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn make_case_concurrent_movable_list_set_same_elem() -> RustOracleVectorCase {
    let a = doc_with_peer(1);
    a.get_movable_list("ml").insert(0, "a").unwrap();
    commit_fixed(&a);

    let base = a.export(ExportMode::all_updates()).unwrap();
    let b = doc_with_peer(2);
    b.import(&base).unwrap();

    a.get_movable_list("ml").set(0, "x").unwrap();
    commit_fixed(&a);

    b.get_movable_list("ml").set(0, "y").unwrap();
    commit_fixed(&b);

    let updates_a = a.export(ExportMode::all_updates()).unwrap();
    let updates_b = b.export(ExportMode::all_updates()).unwrap();

    let oracle = doc_with_peer(999);
    oracle.import(&updates_a).unwrap();
    oracle.import(&updates_b).unwrap();

    RustOracleVectorCase {
        name: "concurrent_movable_list_set_same_elem",
        updates: vec![updates_a, updates_b],
        expected_json: oracle.get_deep_value().to_json(),
    }
}

fn main() {
    let cases = vec![
        make_case_concurrent_list_insert_same_pos(),
        make_case_concurrent_text_insert_same_pos(),
        make_case_concurrent_list_insert_vs_delete(),
        make_case_concurrent_text_insert_vs_delete(),
        make_case_concurrent_movable_list_insert_same_pos(),
        make_case_concurrent_movable_list_move_same_elem(),
        make_case_concurrent_movable_list_set_same_elem(),
    ];

    println!("// Generated by `cargo run -p moon-vectors-gen`");
    println!("let RUST_ORACLE_VECTORS : Array[RustOracleVectorCase] = [");
    for c in cases {
        println!("  {{");
        println!("    name: \"{}\",", c.name);
        println!("    updates_hex: [");
        for u in c.updates {
            println!("      \"{}\",", hex_encode(&u));
        }
        println!("    ],");
        println!("    expected_json: \"{}\",", escape_moonbit_string(&c.expected_json));
        println!("  }},");
    }
    println!("]");
}
