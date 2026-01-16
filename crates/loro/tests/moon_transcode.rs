use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{
    ExpandType, ExportMode, Frontiers, LoroDoc, LoroValue, StyleConfig, StyleConfigMap, Timestamp,
    ToJson, TreeParentId, VersionVector,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

struct MoonCtx {
    node_bin: String,
    cli_js: PathBuf,
}

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

fn moon_ctx() -> Option<&'static MoonCtx> {
    static MOON_CTX: OnceLock<Option<MoonCtx>> = OnceLock::new();
    MOON_CTX
        .get_or_init(|| {
            let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
            let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());

            if !bin_available(&moon_bin, &["version"]) {
                eprintln!("skipping e2e: moon not available (set MOON_BIN)");
                return None;
            }
            if !bin_available(&node_bin, &["--version"]) {
                eprintln!("skipping e2e: node not available (set NODE_BIN)");
                return None;
            }

            let cli_js = match build_moon_cli_js(&moon_bin) {
                Some(p) => p,
                None => {
                    eprintln!("skipping e2e: failed to build MoonBit CLI");
                    return None;
                }
            };

            Some(MoonCtx { node_bin, cli_js })
        })
        .as_ref()
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
    if !out.status.success() {
        anyhow::bail!(
            "node export-jsonschema failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn run_export_deep_json(node_bin: &str, cli_js: &Path, input: &[u8]) -> anyhow::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!(
        "loro-moon-export-deep-json-{}-{ts}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp)?;
    let in_path = tmp.join("in.blob");
    std::fs::write(&in_path, input)?;

    let out = Command::new(node_bin)
        .arg(cli_js)
        .args(["export-deep-json", in_path.to_str().unwrap()])
        .output()?;
    anyhow::ensure!(out.status.success(), "node export-deep-json failed");
    Ok(String::from_utf8(out.stdout)?)
}

fn run_encode_jsonschema(node_bin: &str, cli_js: &Path, input_json: &str) -> anyhow::Result<Vec<u8>> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!(
        "loro-moon-encode-jsonschema-{}-{ts}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp)?;
    let in_path = tmp.join("in.json");
    let out_path = tmp.join("out.blob");
    std::fs::write(&in_path, input_json.as_bytes())?;

    let status = Command::new(node_bin)
        .arg(cli_js)
        .args([
            "encode-jsonschema",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .status()?;
    anyhow::ensure!(status.success(), "node encode-jsonschema failed");

    let out = std::fs::read(&out_path)?;
    Ok(out)
}

fn apply_random_ops(doc: &LoroDoc, seed: u64, ops: usize, commit_every: usize) -> anyhow::Result<()> {
    apply_random_ops_with_peers(doc, seed, ops, commit_every, &[1])
}

fn apply_random_ops_with_peers(
    doc: &LoroDoc,
    seed: u64,
    ops: usize,
    commit_every: usize,
    peer_ids: &[u64],
) -> anyhow::Result<()> {
    let mut rng = StdRng::seed_from_u64(seed);

    let peer_ids = if peer_ids.is_empty() { &[1] } else { peer_ids };

    let mut styles = StyleConfigMap::new();
    styles.insert(
        "bold".into(),
        StyleConfig {
            expand: ExpandType::After,
        },
    );
    styles.insert(
        "link".into(),
        StyleConfig {
            expand: ExpandType::Before,
        },
    );
    doc.config_text_style(styles);

    let mut active_peer = peer_ids[0];
    doc.set_peer_id(active_peer)?;
    let map = doc.get_map("map");
    let list = doc.get_list("list");
    let text = doc.get_text("text");
    let mlist = doc.get_movable_list("mlist");
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);

    // Stable baseline so root containers don't disappear from deep JSON.
    map.insert("keep", 0)?;
    list.insert(0, 0)?;
    text.insert(0, "hi😀")?;
    mlist.insert(0, 0)?;
    let keep_node = tree.create(None)?;
    tree.get_meta(keep_node)?.insert("title", "keep")?;

    // Ensure Text mark/mark_end coverage.
    if text.len_unicode() >= 2 {
        text.mark(0..2, "bold", true)?;
        if text.len_unicode() >= 3 {
            text.mark(1..3, "link", "https://example.com")?;
        }
        text.unmark(0..1, "bold")?;
    }

    // Ensure nested container coverage (container values in map/list/movable_list).
    let child_map = map.insert_container("child_map", loro::LoroMap::new())?;
    child_map.insert("a", 1)?;
    let child_text = child_map.insert_container("t", loro::LoroText::new())?;
    child_text.insert(0, "inner😀")?;

    let child_list = map.insert_container("child_list", loro::LoroList::new())?;
    child_list.insert(0, "x")?;
    let child_mlist = map.insert_container("child_mlist", loro::LoroMovableList::new())?;
    child_mlist.insert(0, 10)?;
    child_mlist.insert(1, 20)?;
    child_mlist.mov(0, 1)?;

    let child_tree = map.insert_container("child_tree", loro::LoroTree::new())?;
    child_tree.enable_fractional_index(0);
    let child_tree_root = child_tree.create(None)?;
    child_tree.get_meta(child_tree_root)?.insert("m", 1)?;

    let maps = [map.clone(), child_map];
    let lists = [list.clone(), child_list];
    let texts = [text.clone(), child_text];
    let mlists = [mlist.clone(), child_mlist];

    struct TreeCtx {
        tree: loro::LoroTree,
        nodes: Vec<loro::TreeID>,
    }
    let mut trees = [
        TreeCtx {
            tree: tree.clone(),
            nodes: vec![keep_node],
        },
        TreeCtx {
            tree: child_tree,
            nodes: vec![child_tree_root],
        },
    ];

    let mut map_keys: Vec<String> = Vec::new();
    let mut child_map_keys: Vec<String> = Vec::new();

    for i in 0..ops {
        // Switch active peer after each commit boundary (when multiple peers are requested).
        if commit_every > 0 && i > 0 && i % commit_every == 0 && peer_ids.len() > 1 {
            active_peer = peer_ids[rng.gen_range(0..peer_ids.len())];
            doc.set_peer_id(active_peer)?;
        }

        let op_type = rng.gen_range(0..18);
        match op_type {
            0 => {
                let key = format!("k{}", rng.gen::<u32>());
                map.insert(&key, rng.gen::<i32>())?;
                map_keys.push(key);
            }
            1 => {
                let key = format!("k{}", rng.gen::<u32>());
                let value = if rng.gen::<bool>() {
                    LoroValue::from(rng.gen::<bool>())
                } else {
                    LoroValue::Null
                };
                map.insert(&key, value)?;
                map_keys.push(key);
            }
            2 => {
                // Insert more value kinds (string/f64/binary) into either root map or child_map.
                let (target, keys) = if rng.gen::<bool>() {
                    (&maps[0], &mut map_keys)
                } else {
                    (&maps[1], &mut child_map_keys)
                };
                let key = format!("v{}", rng.gen::<u32>());
                match rng.gen_range(0..3) {
                    0 => target.insert(&key, "str😀")?,
                    1 => target.insert(&key, rng.gen::<f64>() - 0.5)?,
                    _ => target.insert(&key, vec![0u8, 1, 2, rng.gen::<u8>()])?,
                }
                keys.push(key);
            }
            3 => {
                // Map delete (guarantee it hits an existing key sometimes).
                if !map_keys.is_empty() && rng.gen::<bool>() {
                    let idx = rng.gen_range(0..map_keys.len());
                    let key = map_keys.swap_remove(idx);
                    map.delete(&key)?;
                } else if !child_map_keys.is_empty() {
                    let idx = rng.gen_range(0..child_map_keys.len());
                    let key = child_map_keys.swap_remove(idx);
                    maps[1].delete(&key)?;
                }
            }
            4 => {
                let target = &lists[rng.gen_range(0..lists.len())];
                let index = rng.gen_range(0..=target.len());
                target.insert(index, rng.gen::<i32>())?;
            }
            5 => {
                let target = &lists[rng.gen_range(0..lists.len())];
                if target.len() > 0 {
                    let index = rng.gen_range(0..target.len());
                    let max_len = (target.len() - index).min(3);
                    let len = rng.gen_range(1..=max_len);
                    target.delete(index, len)?;
                }
            }
            6 => {
                let target = &texts[rng.gen_range(0..texts.len())];
                let index = rng.gen_range(0..=target.len_unicode());
                let s = match rng.gen_range(0..8) {
                    0 => "a",
                    1 => "b",
                    2 => "Z",
                    3 => "😀",
                    4 => "中",
                    5 => "ab",
                    6 => "😀!",
                    _ => "!",
                };
                target.insert(index, s)?;
            }
            7 => {
                let target = &texts[rng.gen_range(0..texts.len())];
                let len_u = target.len_unicode();
                if len_u > 0 {
                    let index = rng.gen_range(0..len_u);
                    let max_len = (len_u - index).min(3);
                    let len = rng.gen_range(1..=max_len);
                    target.delete(index, len)?;
                }
            }
            8 => {
                // Text mark/unmark
                let target = &texts[rng.gen_range(0..texts.len())];
                let len_u = target.len_unicode();
                if len_u >= 2 {
                    let start = rng.gen_range(0..len_u - 1);
                    let end = rng.gen_range(start + 1..=len_u);
                    if rng.gen::<bool>() {
                        let key = if rng.gen::<bool>() { "bold" } else { "link" };
                        let value: LoroValue = if key == "bold" {
                            LoroValue::from(true)
                        } else {
                            LoroValue::from("https://loro.dev")
                        };
                        let _ = target.mark(start..end, key, value);
                    } else {
                        let key = if rng.gen::<bool>() { "bold" } else { "link" };
                        let _ = target.unmark(start..end, key);
                    }
                }
            }
            9 => {
                // MovableList insert
                let target = &mlists[rng.gen_range(0..mlists.len())];
                let index = rng.gen_range(0..=target.len());
                target.insert(index, rng.gen::<i32>())?;
            }
            10 => {
                // MovableList delete
                let target = &mlists[rng.gen_range(0..mlists.len())];
                if target.len() > 0 {
                    let index = rng.gen_range(0..target.len());
                    let max_len = (target.len() - index).min(3);
                    let len = rng.gen_range(1..=max_len);
                    target.delete(index, len)?;
                }
            }
            11 => {
                // MovableList set
                let target = &mlists[rng.gen_range(0..mlists.len())];
                if target.len() > 0 {
                    let index = rng.gen_range(0..target.len());
                    target.set(index, rng.gen::<i32>())?;
                }
            }
            12 => {
                // MovableList move
                let target = &mlists[rng.gen_range(0..mlists.len())];
                if target.len() >= 2 {
                    let from = rng.gen_range(0..target.len());
                    let to = rng.gen_range(0..target.len());
                    let _ = target.mov(from, to);
                }
            }
            13 => {
                // Tree create
                let t = &mut trees[rng.gen_range(0..trees.len())];
                let parent = if t.nodes.is_empty() || rng.gen::<bool>() {
                    TreeParentId::Root
                } else {
                    TreeParentId::from(t.nodes[rng.gen_range(0..t.nodes.len())])
                };
                let id = t.tree.create(parent)?;
                t.nodes.push(id);
            }
            14 => {
                // Tree move
                let t = &mut trees[rng.gen_range(0..trees.len())];
                if t.nodes.len() >= 2 {
                    let target = t.nodes[rng.gen_range(0..t.nodes.len())];
                    let parent = if rng.gen::<bool>() {
                        TreeParentId::Root
                    } else {
                        TreeParentId::from(t.nodes[rng.gen_range(0..t.nodes.len())])
                    };
                    let _ = t.tree.mov(target, parent);
                }
            }
            15 => {
                // Tree delete (try to keep at least 1 node around)
                let t = &mut trees[rng.gen_range(0..trees.len())];
                if t.nodes.len() > 1 {
                    let idx = rng.gen_range(0..t.nodes.len());
                    let id = t.nodes.swap_remove(idx);
                    let _ = t.tree.delete(id);
                }
            }
            16 => {
                // Tree meta insert
                let t = &mut trees[rng.gen_range(0..trees.len())];
                if !t.nodes.is_empty() {
                    let id = t.nodes[rng.gen_range(0..t.nodes.len())];
                    if let Ok(meta) = t.tree.get_meta(id) {
                        let key = format!("m{}", rng.gen::<u8>());
                        let _ = meta.insert(&key, rng.gen::<i32>());
                    }
                }
            }
            17 => {
                // Insert container values into sequence containers.
                if rng.gen::<bool>() {
                    let target = &lists[rng.gen_range(0..lists.len())];
                    let index = rng.gen_range(0..=target.len());
                    let _ = target.insert_container(index, loro::LoroMap::new());
                } else {
                    let target = &mlists[rng.gen_range(0..mlists.len())];
                    let index = rng.gen_range(0..=target.len());
                    let _ = target.insert_container(index, loro::LoroText::new());
                }
            }
            _ => unreachable!(),
        }

        if commit_every > 0 && (i + 1) % commit_every == 0 {
            let msg = format!("commit-{} seed={} peer={}", i + 1, seed, active_peer);
            doc.set_next_commit_message(&msg);
            doc.set_next_commit_timestamp(i as Timestamp);
            doc.commit();
        }
    }

    let msg = format!("final seed={seed} ops={ops}");
    doc.set_next_commit_message(&msg);
    doc.set_next_commit_timestamp(ops as Timestamp);
    doc.commit();

    Ok(())
}

fn first_json_diff_path(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
    use serde_json::Value;
    if a == b {
        return None;
    }
    match (a, b) {
        (Value::Object(ao), Value::Object(bo)) => {
            for (k, av) in ao {
                let Some(bv) = bo.get(k) else {
                    return Some(format!("{path}.{k} (missing rhs)"));
                };
                if let Some(p) = first_json_diff_path(av, bv, &format!("{path}.{k}")) {
                    return Some(p);
                }
            }
            for k in bo.keys() {
                if !ao.contains_key(k) {
                    return Some(format!("{path}.{k} (missing lhs)"));
                }
            }
            Some(path.to_string())
        }
        (Value::Array(aa), Value::Array(ba)) => {
            if aa.len() != ba.len() {
                return Some(format!("{path} (len {} != {})", aa.len(), ba.len()));
            }
            for (i, (av, bv)) in aa.iter().zip(ba.iter()).enumerate() {
                if let Some(p) = first_json_diff_path(av, bv, &format!("{path}[{i}]")) {
                    return Some(p);
                }
            }
            Some(path.to_string())
        }
        _ => Some(path.to_string()),
    }
}

fn first_json_diff(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &str,
) -> Option<(String, serde_json::Value, serde_json::Value)> {
    use serde_json::Value;
    if a == b {
        return None;
    }
    match (a, b) {
        (Value::Object(ao), Value::Object(bo)) => {
            for (k, av) in ao {
                let Some(bv) = bo.get(k) else {
                    return Some((format!("{path}.{k} (missing rhs)"), av.clone(), Value::Null));
                };
                if let Some((p, ga, gb)) = first_json_diff(av, bv, &format!("{path}.{k}")) {
                    return Some((p, ga, gb));
                }
            }
            for (k, bv) in bo {
                if !ao.contains_key(k) {
                    return Some((format!("{path}.{k} (missing lhs)"), Value::Null, bv.clone()));
                }
            }
            Some((path.to_string(), a.clone(), b.clone()))
        }
        (Value::Array(aa), Value::Array(ba)) => {
            if aa.len() != ba.len() {
                return Some((
                    format!("{path} (len {} != {})", aa.len(), ba.len()),
                    a.clone(),
                    b.clone(),
                ));
            }
            for (i, (av, bv)) in aa.iter().zip(ba.iter()).enumerate() {
                if let Some((p, ga, gb)) = first_json_diff(av, bv, &format!("{path}[{i}]")) {
                    return Some((p, ga, gb));
                }
            }
            Some((path.to_string(), a.clone(), b.clone()))
        }
        _ => Some((path.to_string(), a.clone(), b.clone())),
    }
}

fn first_bytes_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        if a[i] != b[i] {
            return Some(i);
        }
    }
    (a.len() != b.len()).then_some(min_len)
}

fn assert_updates_jsonschema_matches_rust(doc: &LoroDoc, ctx: &MoonCtx) -> anyhow::Result<()> {
    let start = VersionVector::default();
    let end = doc.oplog_vv();

    let updates_blob = doc.export(ExportMode::Updates {
        from: std::borrow::Cow::Borrowed(&start),
    })?;
    let moon_json = run_export_jsonschema(&ctx.node_bin, &ctx.cli_js, &updates_blob)?;
    let moon_value: serde_json::Value = serde_json::from_str(&moon_json)?;

    let rust_schema = doc.export_json_updates(&start, &end);
    let rust_value = serde_json::to_value(&rust_schema)?;

    anyhow::ensure!(
        moon_value == rust_value,
        "jsonschema mismatch at {:?}",
        first_json_diff_path(&moon_value, &rust_value, "$")
    );
    Ok(())
}

fn assert_snapshot_deep_json_matches_rust(doc: &LoroDoc, ctx: &MoonCtx) -> anyhow::Result<()> {
    let expected = doc.get_deep_value().to_json_value();
    let snapshot_blob = doc.export(ExportMode::Snapshot)?;

    // Ensure Rust snapshot import round-trips for the same op sequence.
    let doc_roundtrip = LoroDoc::new();
    doc_roundtrip.import(&snapshot_blob)?;
    assert_eq!(doc_roundtrip.get_deep_value().to_json_value(), expected);

    let moon_json = run_export_deep_json(&ctx.node_bin, &ctx.cli_js, &snapshot_blob)?;
    let moon_value: serde_json::Value = serde_json::from_str(&moon_json)?;

    assert_eq!(moon_value, expected);
    Ok(())
}

fn apply_curated_ops(doc: &LoroDoc) -> anyhow::Result<()> {
    let mut styles = StyleConfigMap::new();
    styles.insert(
        "bold".into(),
        StyleConfig {
            expand: ExpandType::After,
        },
    );
    styles.insert(
        "link".into(),
        StyleConfig {
            expand: ExpandType::Before,
        },
    );
    doc.config_text_style(styles);

    // Map ops.
    let map = doc.get_map("map");
    map.insert("i32", 1)?;
    map.insert("bool", true)?;
    map.insert("null", LoroValue::Null)?;
    map.insert("str", "hello😀")?;
    map.insert("f64", 1.25f64)?;
    map.insert("bin", vec![0u8, 1, 2, 3])?;
    // Overwrite existing key.
    map.insert("i32", 2)?;
    // Container values in map.
    let child_map = map.insert_container("child_map", loro::LoroMap::new())?;
    child_map.insert("a", 1)?;
    let child_list = map.get_or_create_container("child_list", loro::LoroList::new())?;
    child_list.push("x")?;
    map.delete("null")?;
    // Map clear (but keep non-empty at the end).
    let tmp = map.insert_container("tmp", loro::LoroMap::new())?;
    tmp.insert("k", 1)?;
    tmp.clear()?;
    tmp.insert("k2", 2)?;

    // List ops.
    let list = doc.get_list("list");
    list.insert(0, "a")?;
    list.push("b")?;
    let list_child_text = list.insert_container(2, loro::LoroText::new())?;
    list_child_text.insert(0, "t")?;
    let _ = list.pop()?;
    if list.len() > 0 {
        list.delete(0, 1)?;
    }
    list.clear()?;
    list.push(0)?;
    let list_child_map = list.push_container(loro::LoroMap::new())?;
    list_child_map.insert("k", 1)?;

    // MovableList ops.
    let mlist = doc.get_movable_list("mlist");
    mlist.insert(0, "a")?;
    mlist.push("b")?;
    mlist.set(0, "A")?;
    if mlist.len() >= 2 {
        mlist.mov(0, 1)?;
    }
    let ml_child_text = mlist.insert_container(0, loro::LoroText::new())?;
    ml_child_text.insert(0, "ml")?;
    let ml_set_text = mlist.set_container(0, loro::LoroText::new())?;
    ml_set_text.insert(0, "set")?;
    let _ = mlist.pop()?;
    if mlist.len() > 0 {
        mlist.delete(0, 1)?;
    }
    mlist.clear()?;
    mlist.push(1)?;

    // Text ops.
    let text = doc.get_text("text");
    text.insert(0, "A😀BC")?;
    // Use UTF-8/UTF-16 coordinate APIs at a safe ASCII boundary.
    text.insert_utf8(0, "u8")?;
    text.insert_utf16(0, "u16")?;
    text.delete_utf8(0, 1)?;
    if text.len_unicode() >= 2 {
        text.mark(0..2, "bold", true)?;
        text.mark(1..2, "link", "https://example.com")?;
        text.unmark(0..1, "bold")?;
    }
    if text.len_unicode() >= 2 {
        let _ = text.splice(1, 1, "Z")?;
    }
    if text.len_unicode() > 0 {
        text.delete(0, 1)?;
    }
    text.insert(0, "keep")?;

    // Tree ops (fractional index + ordering moves).
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root_a = tree.create(None)?;
    let root_b = tree.create(None)?;
    let c1 = tree.create(root_a)?;
    let c2 = tree.create_at(root_a, 0)?;
    tree.mov_to(c1, root_a, 1)?;
    tree.mov_after(root_a, root_b)?;
    tree.mov_before(root_a, root_b)?;
    tree.delete(c2)?;

    // Tree meta ops: insert/delete/clear.
    let meta = tree.get_meta(root_a)?;
    meta.insert("title", "A")?;
    meta.insert("num", 1)?;
    meta.delete("num")?;
    meta.clear()?;
    meta.insert("title", "A2")?;

    doc.set_next_commit_message("curated-ops");
    doc.set_next_commit_timestamp(1 as Timestamp);
    doc.commit();
    Ok(())
}

#[test]
fn moon_transcode_e2e() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
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
    let out_updates = run_transcode(&ctx.node_bin, &ctx.cli_js, &updates)?;
    let out_updates2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_updates)?;
    anyhow::ensure!(
        out_updates2 == out_updates,
        "moon transcode not idempotent for FastUpdates at {:?} ({} -> {})",
        first_bytes_diff(&out_updates2, &out_updates),
        out_updates.len(),
        out_updates2.len()
    );
    let doc2 = LoroDoc::new();
    doc2.import(&out_updates).unwrap();
    assert_eq!(doc2.get_deep_value().to_json_value(), expected);

    // JsonSchema export e2e: Rust export (FastUpdates) -> Moon export-jsonschema -> Rust import_json_updates.
    let jsonschema = run_export_jsonschema(&ctx.node_bin, &ctx.cli_js, &updates)?;
    let schema: loro::JsonSchema = serde_json::from_str(&jsonschema)?;
    let doc_json = LoroDoc::new();
    doc_json.import_json_updates(schema).unwrap();
    assert_eq!(doc_json.get_deep_value().to_json_value(), expected);

    // Snapshot e2e (FastSnapshot): Rust export -> Moon transcode -> Rust import.
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let out_snapshot = run_transcode(&ctx.node_bin, &ctx.cli_js, &snapshot)?;
    let out_snapshot2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_snapshot)?;
    anyhow::ensure!(
        out_snapshot2 == out_snapshot,
        "moon transcode not idempotent for Snapshot at {:?} ({} -> {})",
        first_bytes_diff(&out_snapshot2, &out_snapshot),
        out_snapshot.len(),
        out_snapshot2.len()
    );
    let doc3 = LoroDoc::new();
    doc3.import(&out_snapshot).unwrap();
    assert_eq!(doc3.get_deep_value().to_json_value(), expected);

    // SnapshotAt e2e (FastSnapshot): decode snapshot at an earlier version.
    let snapshot_at = doc
        .export(ExportMode::SnapshotAt {
            version: std::borrow::Cow::Borrowed(&frontiers_v1),
        })
        .unwrap();
    let out_snapshot_at = run_transcode(&ctx.node_bin, &ctx.cli_js, &snapshot_at)?;
    let out_snapshot_at2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_snapshot_at)?;
    anyhow::ensure!(
        out_snapshot_at2 == out_snapshot_at,
        "moon transcode not idempotent for SnapshotAt at {:?} ({} -> {})",
        first_bytes_diff(&out_snapshot_at2, &out_snapshot_at),
        out_snapshot_at.len(),
        out_snapshot_at2.len()
    );
    let doc_at = LoroDoc::new();
    doc_at.import(&out_snapshot_at).unwrap();
    assert_eq!(doc_at.get_deep_value().to_json_value(), expected_v1);

    // StateOnly e2e (FastSnapshot): state at an earlier version with minimal history.
    let state_only = doc
        .export(ExportMode::StateOnly(Some(std::borrow::Cow::Borrowed(
            &frontiers_v1,
        ))))
        .unwrap();
    let out_state_only = run_transcode(&ctx.node_bin, &ctx.cli_js, &state_only)?;
    let out_state_only2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_state_only)?;
    anyhow::ensure!(
        out_state_only2 == out_state_only,
        "moon transcode not idempotent for StateOnly at {:?} ({} -> {})",
        first_bytes_diff(&out_state_only2, &out_state_only),
        out_state_only.len(),
        out_state_only2.len()
    );
    let doc_state_only = LoroDoc::new();
    doc_state_only.import(&out_state_only).unwrap();
    assert_eq!(doc_state_only.get_deep_value().to_json_value(), expected_v1);

    // ShallowSnapshot e2e (FastSnapshot): full current state + partial history since v1.
    let shallow = doc
        .export(ExportMode::ShallowSnapshot(std::borrow::Cow::Borrowed(
            &frontiers_v1,
        )))
        .unwrap();
    let out_shallow = run_transcode(&ctx.node_bin, &ctx.cli_js, &shallow)?;
    let out_shallow2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_shallow)?;
    anyhow::ensure!(
        out_shallow2 == out_shallow,
        "moon transcode not idempotent for ShallowSnapshot at {:?} ({} -> {})",
        first_bytes_diff(&out_shallow2, &out_shallow),
        out_shallow.len(),
        out_shallow2.len()
    );
    let doc_shallow = LoroDoc::new();
    doc_shallow.import(&out_shallow).unwrap();
    assert_eq!(doc_shallow.get_deep_value().to_json_value(), expected);

    // Updates(from vv) e2e: snapshot_at(v1) + updates(vv_v1) => latest.
    let vv_v1: VersionVector = doc.frontiers_to_vv(&frontiers_v1).unwrap();
    let updates_since_v1 = doc.export(ExportMode::Updates {
        from: std::borrow::Cow::Borrowed(&vv_v1),
    })?;
    let out_updates_since_v1 = run_transcode(&ctx.node_bin, &ctx.cli_js, &updates_since_v1)?;
    let out_updates_since_v12 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_updates_since_v1)?;
    anyhow::ensure!(
        out_updates_since_v12 == out_updates_since_v1,
        "moon transcode not idempotent for Updates(from) at {:?} ({} -> {})",
        first_bytes_diff(&out_updates_since_v12, &out_updates_since_v1),
        out_updates_since_v1.len(),
        out_updates_since_v12.len()
    );
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
    let out_updates_b = run_transcode(&ctx.node_bin, &ctx.cli_js, &updates_b)?;
    let out_updates_b2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_updates_b)?;
    anyhow::ensure!(
        out_updates_b2 == out_updates_b,
        "moon transcode not idempotent for multi-peer Updates at {:?} ({} -> {})",
        first_bytes_diff(&out_updates_b2, &out_updates_b),
        out_updates_b.len(),
        out_updates_b2.len()
    );
    let doc_c = LoroDoc::new();
    doc_c.import(&out_updates_b).unwrap();
    assert_eq!(doc_c.get_deep_value().to_json_value(), expected_b);

    Ok(())
}

#[test]
fn moon_edge_varints_and_lengths() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    // Stress:
    // - LEB128 / varint boundaries (lengths >= 128, peer tables >= 128).
    // - Big peer IDs (JS-safe boundary) carried through JSON schema.
    // - String/binary lengths at 127/128.
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let list = doc.get_list("l");

    // Commit #1: one change with many keys/ops.
    doc.set_peer_id(1)?;
    doc.set_next_commit_timestamp(-1 as Timestamp);

    map.insert("", "empty-key")?;
    map.insert("s127", "a".repeat(127))?;
    map.insert("s128", "b".repeat(128))?;
    map.insert("bin0", Vec::<u8>::new())?;
    map.insert("bin127", vec![7u8; 127])?;
    map.insert("bin128", vec![8u8; 128])?;

    for i in 0..130u32 {
        let key = format!("k{i:03}");
        map.insert(&key, i as i64)?;
        list.push(i as i64)?;
    }

    // Root container name length boundaries (UTF-8 byte length).
    let root_127 = "r".repeat(127);
    doc.get_map(root_127.as_str()).insert("x", 1)?;
    let root_emoji = "😀".repeat(40); // 160 UTF-8 bytes
    doc.get_list(root_emoji.as_str()).push("y")?;

    doc.commit();

    // More peers to force peer-index varints (len >= 128).
    for peer in 2u64..=130u64 {
        doc.set_peer_id(peer)?;
        if peer == 2 {
            doc.set_next_commit_message("");
            doc.set_next_commit_timestamp(0 as Timestamp);
        } else {
            doc.set_next_commit_timestamp(peer as Timestamp);
        }
        let key = format!("p{peer:03}");
        map.insert(&key, peer as i64)?;
        doc.commit();
    }

    // Big peer ID (forces bigint path in JS).
    let big_peer: u64 = 9_007_199_254_740_993; // 2^53 + 1
    doc.set_peer_id(big_peer)?;
    doc.set_next_commit_message("big-peer");
    doc.set_next_commit_timestamp(1_700_000_000 as Timestamp);
    map.insert("big_peer", big_peer as i64)?;
    doc.commit();

    // Decode correctness (Moon export-deep-json / export-jsonschema).
    assert_snapshot_deep_json_matches_rust(&doc, ctx)?;
    assert_updates_jsonschema_matches_rust(&doc, ctx)?;

    // Encode correctness: Moon transcode is deterministic (idempotent) and importable by Rust.
    let snapshot = doc.export(ExportMode::Snapshot)?;
    let out_snapshot = run_transcode(&ctx.node_bin, &ctx.cli_js, &snapshot)?;
    let out_snapshot2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_snapshot)?;
    anyhow::ensure!(
        out_snapshot2 == out_snapshot,
        "moon transcode not idempotent for edge Snapshot at {:?} ({} -> {})",
        first_bytes_diff(&out_snapshot2, &out_snapshot),
        out_snapshot.len(),
        out_snapshot2.len()
    );
    let doc_from_snapshot = LoroDoc::new();
    doc_from_snapshot.import(&out_snapshot).unwrap();
    assert_eq!(
        doc_from_snapshot.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );
    let updates = doc.export(ExportMode::all_updates())?;
    let out_updates = run_transcode(&ctx.node_bin, &ctx.cli_js, &updates)?;
    let out_updates2 = run_transcode(&ctx.node_bin, &ctx.cli_js, &out_updates)?;
    anyhow::ensure!(
        out_updates2 == out_updates,
        "moon transcode not idempotent for edge Updates at {:?} ({} -> {})",
        first_bytes_diff(&out_updates2, &out_updates),
        out_updates.len(),
        out_updates2.len()
    );
    let doc_from_updates = LoroDoc::new();
    doc_from_updates.import(&out_updates).unwrap();
    assert_eq!(
        doc_from_updates.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    // Roundtrip: Moon encode-jsonschema output must be importable by Rust (large peer/key tables).
    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let schema = doc.export_json_updates(&start, &end);
    let json = serde_json::to_string(&schema)?;
    let out_blob = run_encode_jsonschema(&ctx.node_bin, &ctx.cli_js, &json)?;
    let doc2 = LoroDoc::new();
    doc2.import(&out_blob).unwrap();
    let got = doc2.get_deep_value().to_json_value();
    let expected = doc.get_deep_value().to_json_value();
    anyhow::ensure!(
        got == expected,
        "encode-jsonschema state mismatch: {:?}",
        first_json_diff(&got, &expected, "$")
    );

    Ok(())
}

#[test]
fn moon_decode_ops_text_insert() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let peer: u64 = 0x0102_0304_0506_0708;
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_text("t").insert(0, "123").unwrap();
    doc.commit();

    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let json = run_decode_updates(&ctx.node_bin, &ctx.cli_js, &updates)?;
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
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let peer: u64 = 0x0102_0304_0506_0708;
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_text("t").insert(0, "123").unwrap();
    doc.commit();

    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let json = run_export_jsonschema(&ctx.node_bin, &ctx.cli_js, &updates)?;
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
fn moon_encode_jsonschema_text_insert() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let peer: u64 = 0x0102_0304_0506_0708;
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_text("t").insert(0, "123").unwrap();
    doc.commit();
    let expected = doc.get_deep_value().to_json_value();

    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let schema = doc.export_json_updates(&start, &end);
    let json = serde_json::to_string(&schema)?;

    let out_blob = run_encode_jsonschema(&ctx.node_bin, &ctx.cli_js, &json)?;
    let doc2 = LoroDoc::new();
    doc2.import(&out_blob).unwrap();
    assert_eq!(doc2.get_deep_value().to_json_value(), expected);

    Ok(())
}

#[test]
fn moon_export_jsonschema_updates_since_v1() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
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

    let json = run_export_jsonschema(&ctx.node_bin, &ctx.cli_js, &updates_since_v1)?;
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
    let Some(ctx) = moon_ctx() else {
        return Ok(());
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
    let json = run_export_jsonschema(&ctx.node_bin, &ctx.cli_js, &updates_b)?;
    let schema: loro::JsonSchema = serde_json::from_str(&json)?;

    let mut peers = schema.peers.clone().unwrap_or_default();
    peers.sort();
    assert_eq!(peers, vec![1, 2]);

    let doc_c = LoroDoc::new();
    doc_c.import_json_updates(schema).unwrap();
    assert_eq!(doc_c.get_deep_value().to_json_value(), expected_b);

    Ok(())
}

#[test]
fn moon_golden_updates_jsonschema_matches_rust() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let seed = 42;
    let doc = LoroDoc::new();
    apply_random_ops(&doc, seed, 200, 20)?;
    assert_updates_jsonschema_matches_rust(&doc, ctx)
}

#[test]
fn moon_golden_snapshot_deep_json_matches_rust() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let seed = 1337;
    let doc = LoroDoc::new();
    apply_random_ops(&doc, seed, 200, 20)?;
    assert_snapshot_deep_json_matches_rust(&doc, ctx)
}

fn golden_random_updates(seed: u64, ops: usize, commit_every: usize, peers: &[u64]) -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };
    let doc = LoroDoc::new();
    apply_random_ops_with_peers(&doc, seed, ops, commit_every, peers)?;
    assert_updates_jsonschema_matches_rust(&doc, ctx)
}

fn golden_random_snapshot(seed: u64, ops: usize, commit_every: usize, peers: &[u64]) -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };
    let doc = LoroDoc::new();
    apply_random_ops_with_peers(&doc, seed, ops, commit_every, peers)?;
    assert_snapshot_deep_json_matches_rust(&doc, ctx)
}

#[test]
fn moon_curated_updates_jsonschema_matches_rust() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };
    let doc = LoroDoc::new();
    apply_curated_ops(&doc)?;
    assert_updates_jsonschema_matches_rust(&doc, ctx)
}

#[test]
fn moon_curated_snapshot_deep_json_matches_rust() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };
    let doc = LoroDoc::new();
    apply_curated_ops(&doc)?;
    assert_snapshot_deep_json_matches_rust(&doc, ctx)
}

#[test]
fn moon_golden_updates_seed_0() -> anyhow::Result<()> {
    golden_random_updates(0, 200, 20, &[1])
}

#[test]
fn moon_golden_updates_seed_1() -> anyhow::Result<()> {
    golden_random_updates(1, 200, 20, &[1])
}

#[test]
fn moon_golden_updates_seed_2() -> anyhow::Result<()> {
    golden_random_updates(2, 200, 20, &[1])
}

#[test]
fn moon_golden_updates_seed_3() -> anyhow::Result<()> {
    golden_random_updates(3, 200, 20, &[1])
}

#[test]
fn moon_golden_updates_multi_peer_seed_7() -> anyhow::Result<()> {
    golden_random_updates(7, 250, 25, &[1, 2, 3])
}

#[test]
fn moon_golden_snapshot_seed_0() -> anyhow::Result<()> {
    golden_random_snapshot(0, 200, 20, &[1])
}

#[test]
fn moon_golden_snapshot_seed_1() -> anyhow::Result<()> {
    golden_random_snapshot(1, 200, 20, &[1])
}

#[test]
fn moon_golden_snapshot_seed_2() -> anyhow::Result<()> {
    golden_random_snapshot(2, 200, 20, &[1])
}

#[test]
fn moon_golden_snapshot_seed_3() -> anyhow::Result<()> {
    golden_random_snapshot(3, 200, 20, &[1])
}

#[test]
fn moon_golden_snapshot_multi_peer_seed_7() -> anyhow::Result<()> {
    golden_random_snapshot(7, 250, 25, &[1, 2, 3])
}

#[cfg(feature = "counter")]
#[test]
fn moon_counter_snapshot_deep_json_matches_rust() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let counter = map.insert_container("c", loro::LoroCounter::new())?;
    counter.increment(1.0)?;
    counter.decrement(0.5)?;
    doc.set_next_commit_message("counter");
    doc.set_next_commit_timestamp(1 as Timestamp);
    doc.commit();

    assert_snapshot_deep_json_matches_rust(&doc, ctx)
}

#[cfg(feature = "counter")]
#[test]
fn moon_counter_updates_jsonschema_matches_rust() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let counter = map.insert_container("c", loro::LoroCounter::new())?;
    counter.increment(1.0)?;
    counter.decrement(0.5)?;
    doc.set_next_commit_message("counter");
    doc.set_next_commit_timestamp(1 as Timestamp);
    doc.commit();

    assert_updates_jsonschema_matches_rust(&doc, ctx)
}

#[cfg(feature = "counter")]
#[test]
fn moon_encode_jsonschema_counter() -> anyhow::Result<()> {
    let Some(ctx) = moon_ctx() else {
        return Ok(());
    };

    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let counter = map.insert_container("c", loro::LoroCounter::new())?;
    counter.increment(1.0)?;
    counter.decrement(0.5)?;
    doc.set_next_commit_message("counter");
    doc.set_next_commit_timestamp(1 as Timestamp);
    doc.commit();
    let expected = doc.get_deep_value().to_json_value();

    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let schema = doc.export_json_updates(&start, &end);
    let json = serde_json::to_string(&schema)?;

    let out_blob = run_encode_jsonschema(&ctx.node_bin, &ctx.cli_js, &json)?;
    let doc2 = LoroDoc::new();
    doc2.import(&out_blob).unwrap();
    assert_eq!(doc2.get_deep_value().to_json_value(), expected);

    Ok(())
}
