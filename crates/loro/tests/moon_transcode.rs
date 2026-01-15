use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{ExportMode, Frontiers, LoroDoc, Timestamp, ToJson, VersionVector};

fn bin_available(bin: &str, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn repo_root() -> PathBuf {
    // crates/loro -> crates -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn build_moon_cli_js(moon_bin: &str) -> Option<PathBuf> {
    let root = repo_root();
    let moon_dir = root.join("moon");
    let status = Command::new(moon_bin)
        .current_dir(&moon_dir)
        .args(["build", "--target", "js", "--release", "cmd/loro_codec_cli"])
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    Some(
        moon_dir
            .join("_build/js/release/build/cmd/loro_codec_cli/loro_codec_cli.js"),
    )
}

fn run_transcode(node_bin: &str, cli_js: &Path, input: &[u8]) -> anyhow::Result<Vec<u8>> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!(
        "loro-moon-transcode-{}-{ts}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp)?;
    let in_path = tmp.join("in.blob");
    let out_path = tmp.join("out.blob");
    std::fs::write(&in_path, input)?;

    let status = Command::new(node_bin)
        .arg(cli_js)
        .args(["transcode", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .status()?;
    anyhow::ensure!(status.success(), "node transcode failed");

    let out = std::fs::read(&out_path)?;
    Ok(out)
}

fn run_decode_updates(node_bin: &str, cli_js: &Path, input: &[u8]) -> anyhow::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!(
        "loro-moon-decode-updates-{}-{ts}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp)?;
    let in_path = tmp.join("in.blob");
    std::fs::write(&in_path, input)?;

    let out = Command::new(node_bin)
        .arg(cli_js)
        .args(["decode-updates", in_path.to_str().unwrap()])
        .output()?;
    anyhow::ensure!(out.status.success(), "node decode-updates failed");
    Ok(String::from_utf8(out.stdout)?)
}

fn run_export_jsonschema(node_bin: &str, cli_js: &Path, input: &[u8]) -> anyhow::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!(
        "loro-moon-export-jsonschema-{}-{ts}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp)?;
    let in_path = tmp.join("in.blob");
    std::fs::write(&in_path, input)?;

    let out = Command::new(node_bin)
        .arg(cli_js)
        .args(["export-jsonschema", in_path.to_str().unwrap()])
        .output()?;
    anyhow::ensure!(out.status.success(), "node export-jsonschema failed");
    Ok(String::from_utf8(out.stdout)?)
}

#[test]
fn moon_transcode_e2e() -> anyhow::Result<()> {
    let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());

    if !bin_available(&moon_bin, &["version"]) {
        eprintln!("skipping e2e: moon not available (set MOON_BIN)");
        return Ok(());
    }
    if !bin_available(&node_bin, &["--version"]) {
        eprintln!("skipping e2e: node not available (set NODE_BIN)");
        return Ok(());
    }

    let cli_js = match build_moon_cli_js(&moon_bin) {
        Some(p) => p,
        None => {
            eprintln!("skipping e2e: failed to build MoonBit CLI");
            return Ok(());
        }
    };

    // Build a doc that exercises multiple op kinds.
    let doc = LoroDoc::new();

    // Commit #1 (with msg/timestamp) to create an intermediate frontiers for SnapshotAt/StateOnly.
    doc.set_next_commit_message("commit-1");
    doc.set_next_commit_timestamp(1 as Timestamp);

    doc.get_map("map").insert("x", 1).unwrap();
    doc.get_map("map").insert("y", true).unwrap();

    let list = doc.get_list("list");
    list.insert(0, 1).unwrap();
    list.insert(1, 2).unwrap();
    list.delete(0, 1).unwrap();

    let mlist = doc.get_movable_list("mlist");
    mlist.insert(0, 10).unwrap();
    mlist.insert(1, 20).unwrap();
    mlist.mov(0, 1).unwrap();
    mlist.set(0, 99).unwrap();
    mlist.delete(0, 1).unwrap();

    let text = doc.get_text("text");
    text.insert(0, "a😀b").unwrap();
    text.insert(3, "!").unwrap();
    text.delete(1, 1).unwrap();

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let n1 = tree.create(None).unwrap();
    tree.get_meta(n1).unwrap().insert("title", "A").unwrap();
    let n2 = tree.create(None).unwrap();
    tree.get_meta(n2).unwrap().insert("title", "B").unwrap();
    tree.mov_after(n1, n2).unwrap();
    tree.delete(n2).unwrap();

    doc.commit();
    let frontiers_v1: Frontiers = doc.state_frontiers();
    let expected_v1 = doc.get_deep_value().to_json_value();

    // Commit #2 to create a newer version.
    doc.set_next_commit_message("commit-2 😀");
    doc.set_next_commit_timestamp(2 as Timestamp);
    doc.get_map("map").insert("z", 123).unwrap();
    doc.get_text("text").insert(0, "Z").unwrap();
    doc.commit();
    let expected = doc.get_deep_value().to_json_value();

    // Updates e2e (FastUpdates): Rust export -> Moon transcode -> Rust import.
    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let out_updates = run_transcode(&node_bin, &cli_js, &updates)?;
    let doc2 = LoroDoc::new();
    doc2.import(&out_updates).unwrap();
    assert_eq!(doc2.get_deep_value().to_json_value(), expected);

    // JsonSchema export e2e: Rust export (FastUpdates) -> Moon export-jsonschema -> Rust import_json_updates.
    let jsonschema = run_export_jsonschema(&node_bin, &cli_js, &updates)?;
    let schema: loro::JsonSchema = serde_json::from_str(&jsonschema)?;
    let doc_json = LoroDoc::new();
    doc_json.import_json_updates(schema).unwrap();
    assert_eq!(doc_json.get_deep_value().to_json_value(), expected);

    // Snapshot e2e (FastSnapshot): Rust export -> Moon transcode -> Rust import.
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let out_snapshot = run_transcode(&node_bin, &cli_js, &snapshot)?;
    let doc3 = LoroDoc::new();
    doc3.import(&out_snapshot).unwrap();
    assert_eq!(doc3.get_deep_value().to_json_value(), expected);

    // SnapshotAt e2e (FastSnapshot): decode snapshot at an earlier version.
    let snapshot_at = doc
        .export(ExportMode::SnapshotAt {
            version: std::borrow::Cow::Borrowed(&frontiers_v1),
        })
        .unwrap();
    let out_snapshot_at = run_transcode(&node_bin, &cli_js, &snapshot_at)?;
    let doc_at = LoroDoc::new();
    doc_at.import(&out_snapshot_at).unwrap();
    assert_eq!(doc_at.get_deep_value().to_json_value(), expected_v1);

    // StateOnly e2e (FastSnapshot): state at an earlier version with minimal history.
    let state_only = doc
        .export(ExportMode::StateOnly(Some(std::borrow::Cow::Borrowed(
            &frontiers_v1,
        ))))
        .unwrap();
    let out_state_only = run_transcode(&node_bin, &cli_js, &state_only)?;
    let doc_state_only = LoroDoc::new();
    doc_state_only.import(&out_state_only).unwrap();
    assert_eq!(doc_state_only.get_deep_value().to_json_value(), expected_v1);

    // ShallowSnapshot e2e (FastSnapshot): full current state + partial history since v1.
    let shallow = doc
        .export(ExportMode::ShallowSnapshot(std::borrow::Cow::Borrowed(
            &frontiers_v1,
        )))
        .unwrap();
    let out_shallow = run_transcode(&node_bin, &cli_js, &shallow)?;
    let doc_shallow = LoroDoc::new();
    doc_shallow.import(&out_shallow).unwrap();
    assert_eq!(doc_shallow.get_deep_value().to_json_value(), expected);

    // Updates(from vv) e2e: snapshot_at(v1) + updates(vv_v1) => latest.
    let vv_v1: VersionVector = doc.frontiers_to_vv(&frontiers_v1).unwrap();
    let updates_since_v1 = doc.export(ExportMode::Updates {
        from: std::borrow::Cow::Borrowed(&vv_v1),
    })?;
    let out_updates_since_v1 = run_transcode(&node_bin, &cli_js, &updates_since_v1)?;
    let doc_from_v1 = LoroDoc::new();
    doc_from_v1.import(&out_snapshot_at).unwrap();
    doc_from_v1.import(&out_updates_since_v1).unwrap();
    assert_eq!(doc_from_v1.get_deep_value().to_json_value(), expected);

    // Multi-peer e2e: updates should include >1 peer.
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    doc_a.set_next_commit_message("A-1");
    doc_a.get_map("m").insert("a", 1).unwrap();
    doc_a.commit();

    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    doc_b.import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc_b.set_next_commit_message("B-1");
    doc_b.get_map("m").insert("b", 2).unwrap();
    doc_b.commit();
    let expected_b = doc_b.get_deep_value().to_json_value();

    let updates_b = doc_b.export(ExportMode::all_updates()).unwrap();
    let out_updates_b = run_transcode(&node_bin, &cli_js, &updates_b)?;
    let doc_c = LoroDoc::new();
    doc_c.import(&out_updates_b).unwrap();
    assert_eq!(doc_c.get_deep_value().to_json_value(), expected_b);

    Ok(())
}

#[test]
fn moon_decode_ops_text_insert() -> anyhow::Result<()> {
    let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());

    if !bin_available(&moon_bin, &["version"]) {
        eprintln!("skipping decode ops: moon not available (set MOON_BIN)");
        return Ok(());
    }
    if !bin_available(&node_bin, &["--version"]) {
        eprintln!("skipping decode ops: node not available (set NODE_BIN)");
        return Ok(());
    }

    let cli_js = match build_moon_cli_js(&moon_bin) {
        Some(p) => p,
        None => {
            eprintln!("skipping decode ops: failed to build MoonBit CLI");
            return Ok(());
        }
    };

    let peer: u64 = 0x0102_0304_0506_0708;
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_text("t").insert(0, "123").unwrap();
    doc.commit();

    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let json = run_decode_updates(&node_bin, &cli_js, &updates)?;
    let v: serde_json::Value = serde_json::from_str(&json)?;

    let changes = v
        .get("changes")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("missing changes array"))?;

    let expected_container = "cid:root-t:Text";
    let expected_peer_suffix = format!("@{peer}");

    let mut found = false;
    for c in changes {
        let Some(id) = c.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        if !id.ends_with(&expected_peer_suffix) {
            continue;
        }
        let Some(ops) = c.get("ops").and_then(|x| x.as_array()) else {
            continue;
        };
        for op in ops {
            if op.get("container").and_then(|x| x.as_str()) != Some(expected_container) {
                continue;
            }
            let Some(insert) = op
                .get("content")
                .and_then(|x| x.get("Text"))
                .and_then(|x| x.get("Insert"))
            else {
                continue;
            };
            if insert.get("pos").and_then(|x| x.as_i64()) == Some(0)
                && insert.get("text").and_then(|x| x.as_str()) == Some("123")
            {
                found = true;
                break;
            }
        }
    }

    anyhow::ensure!(found, "expected Text insert op not found in Moon decode output");
    Ok(())
}

#[test]
fn moon_export_jsonschema_text_insert() -> anyhow::Result<()> {
    let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());

    if !bin_available(&moon_bin, &["version"]) {
        eprintln!("skipping jsonschema export: moon not available (set MOON_BIN)");
        return Ok(());
    }
    if !bin_available(&node_bin, &["--version"]) {
        eprintln!("skipping jsonschema export: node not available (set NODE_BIN)");
        return Ok(());
    }

    let cli_js = match build_moon_cli_js(&moon_bin) {
        Some(p) => p,
        None => {
            eprintln!("skipping jsonschema export: failed to build MoonBit CLI");
            return Ok(());
        }
    };

    let peer: u64 = 0x0102_0304_0506_0708;
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_text("t").insert(0, "123").unwrap();
    doc.commit();

    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let json = run_export_jsonschema(&node_bin, &cli_js, &updates)?;
    let schema: loro::JsonSchema = serde_json::from_str(&json)?;

    assert_eq!(schema.schema_version, 1);
    assert_eq!(schema.peers.as_deref(), Some(&[peer][..]));

    let expected_container = "cid:root-t:Text";
    let mut found = false;
    for change in &schema.changes {
        // After peer-compression, change IDs use peer indices (so the only peer here is 0).
        if change.id.peer != 0 {
            continue;
        }
        for op in &change.ops {
            if op.container.to_string() != expected_container {
                continue;
            }
            match &op.content {
                loro::JsonOpContent::Text(loro::JsonTextOp::Insert { pos, text }) => {
                    if *pos == 0 && text == "123" {
                        found = true;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    anyhow::ensure!(found, "expected Text insert op not found in Moon jsonschema output");

    // Roundtrip: Moon jsonschema output must be importable by Rust.
    let doc2 = LoroDoc::new();
    doc2.import_json_updates(schema).unwrap();
    assert_eq!(doc2.get_deep_value().to_json_value(), doc.get_deep_value().to_json_value());

    Ok(())
}

#[test]
fn moon_export_jsonschema_updates_since_v1() -> anyhow::Result<()> {
    let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());

    if !bin_available(&moon_bin, &["version"]) {
        eprintln!("skipping jsonschema export: moon not available (set MOON_BIN)");
        return Ok(());
    }
    if !bin_available(&node_bin, &["--version"]) {
        eprintln!("skipping jsonschema export: node not available (set NODE_BIN)");
        return Ok(());
    }

    let cli_js = match build_moon_cli_js(&moon_bin) {
        Some(p) => p,
        None => {
            eprintln!("skipping jsonschema export: failed to build MoonBit CLI");
            return Ok(());
        }
    };

    let peer: u64 = 100;
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;

    doc.get_text("t").insert(0, "a").unwrap();
    doc.commit();
    let frontiers_v1: Frontiers = doc.state_frontiers();

    doc.get_text("t").insert(1, "b").unwrap();
    doc.get_map("m").insert("k", 1).unwrap();
    doc.commit();
    let expected = doc.get_deep_value().to_json_value();

    let vv_v1: VersionVector = doc.frontiers_to_vv(&frontiers_v1).unwrap();
    let updates_since_v1 = doc.export(ExportMode::Updates {
        from: std::borrow::Cow::Borrowed(&vv_v1),
    })?;

    let json = run_export_jsonschema(&node_bin, &cli_js, &updates_since_v1)?;
    let schema: loro::JsonSchema = serde_json::from_str(&json)?;

    // `start_version` should match the starting frontiers of this range.
    assert_eq!(schema.start_version, frontiers_v1);

    // Apply on top of SnapshotAt(v1) should yield the latest state.
    let base = LoroDoc::new();
    base.import(
        &doc.export(ExportMode::SnapshotAt {
            version: std::borrow::Cow::Borrowed(&frontiers_v1),
        })?,
    )?;
    base.import_json_updates(schema).unwrap();
    assert_eq!(base.get_deep_value().to_json_value(), expected);

    Ok(())
}

#[test]
fn moon_export_jsonschema_multi_peer() -> anyhow::Result<()> {
    let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());

    if !bin_available(&moon_bin, &["version"]) {
        eprintln!("skipping jsonschema export: moon not available (set MOON_BIN)");
        return Ok(());
    }
    if !bin_available(&node_bin, &["--version"]) {
        eprintln!("skipping jsonschema export: node not available (set NODE_BIN)");
        return Ok(());
    }

    let cli_js = match build_moon_cli_js(&moon_bin) {
        Some(p) => p,
        None => {
            eprintln!("skipping jsonschema export: failed to build MoonBit CLI");
            return Ok(());
        }
    };

    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    doc_a.get_map("m").insert("a", 1).unwrap();
    doc_a.commit();

    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    doc_b.import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc_b.get_map("m").insert("b", 2).unwrap();
    doc_b.commit();
    let expected_b = doc_b.get_deep_value().to_json_value();

    let updates_b = doc_b.export(ExportMode::all_updates()).unwrap();
    let json = run_export_jsonschema(&node_bin, &cli_js, &updates_b)?;
    let schema: loro::JsonSchema = serde_json::from_str(&json)?;

    let mut peers = schema.peers.clone().unwrap_or_default();
    peers.sort();
    assert_eq!(peers, vec![1, 2]);

    let doc_c = LoroDoc::new();
    doc_c.import_json_updates(schema).unwrap();
    assert_eq!(doc_c.get_deep_value().to_json_value(), expected_b);

    Ok(())
}
