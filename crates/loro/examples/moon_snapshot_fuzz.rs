use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{
    ExpandType, ExportMode, LoroDoc, LoroValue, StyleConfig, StyleConfigMap, Timestamp, ToJson,
    TreeParentId,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::Value;

fn usage() -> ! {
    eprintln!(
        r#"moon_snapshot_fuzz (loro)

Randomly generate Loro ops in Rust, export a FastSnapshot, then ask MoonBit to
decode the snapshot and output deep JSON. The deep JSON must match Rust `get_deep_value()`.

Usage:
  MOON_BIN=~/.moon/bin/moon NODE_BIN=node \
  cargo run -p loro --example moon_snapshot_fuzz -- \
    --iters <n> [--seed <u64>] [--ops <n>] [--commit-every <n>] [--peers <n>] [--out-dir <dir>]

If a mismatch happens, this tool writes a reproducible case into:
  <out-dir>/case-<seed>/

"#
    );
    std::process::exit(2);
}

fn parse_arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|w| (w[0] == name).then_some(w[1].as_str()))
}

fn parse_usize(args: &[String], name: &str, default: usize) -> usize {
    match parse_arg_value(args, name) {
        None => default,
        Some(v) => v.parse().unwrap_or_else(|_| usage()),
    }
}

fn parse_u64(args: &[String], name: &str, default: u64) -> u64 {
    match parse_arg_value(args, name) {
        None => default,
        Some(v) => v.parse().unwrap_or_else(|_| usage()),
    }
}

fn parse_out_dir(args: &[String]) -> PathBuf {
    parse_arg_value(args, "--out-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("moon_snapshot_fuzz_artifacts"))
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

fn build_moon_cli_js(moon_bin: &str) -> anyhow::Result<PathBuf> {
    let root = repo_root();
    let moon_dir = root.join("moon");
    let status = Command::new(moon_bin)
        .current_dir(&moon_dir)
        .args(["build", "--target", "js", "--release", "cmd/loro_codec_cli"])
        .status()?;
    anyhow::ensure!(status.success(), "failed to build MoonBit CLI");
    Ok(moon_dir.join("_build/js/release/build/cmd/loro_codec_cli/loro_codec_cli.js"))
}

fn run_export_deep_json(node_bin: &str, cli_js: &Path, input: &[u8]) -> anyhow::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!(
        "loro-moon-snapshot-fuzz-{}-{ts}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp)?;
    let in_path = tmp.join("snapshot.blob");
    std::fs::write(&in_path, input)?;

    let out = Command::new(node_bin)
        .arg(cli_js)
        .args(["export-deep-json", in_path.to_str().unwrap()])
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "node export-deep-json failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8(out.stdout)?)
}

fn write_json(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    std::fs::write(path, s)?;
    Ok(())
}

fn json_number_eq(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    if a.is_f64() || b.is_f64() {
        match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    } else {
        a == b
    }
}

fn json_value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => json_number_eq(x, y),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(xs), Value::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys.iter()).all(|(x, y)| json_value_eq(x, y))
        }
        (Value::Object(mx), Value::Object(my)) => {
            if mx.len() != my.len() {
                return false;
            }
            mx.iter()
                .all(|(k, vx)| my.get(k).is_some_and(|vy| json_value_eq(vx, vy)))
        }
        _ => false,
    }
}

fn apply_random_ops(
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

    // Counter (always enabled by default in this repo).
    let counter = map.insert_container("counter", loro::LoroCounter::new())?;

    // Stable baseline so root containers don't disappear from deep JSON.
    map.insert("keep", 0)?;
    list.insert(0, 0)?;
    text.insert(0, "hi😀")?;
    mlist.insert(0, 0)?;
    counter.increment(0.0)?;
    let keep_node = tree.create(None)?;
    tree.get_meta(keep_node)?.insert("title", "keep")?;

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

    let counters = [counter];
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

        let op_type = rng.gen_range(0..20);
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
                    let _ = map.delete(&key);
                } else if !child_map_keys.is_empty() {
                    let idx = rng.gen_range(0..child_map_keys.len());
                    let key = child_map_keys.swap_remove(idx);
                    let _ = maps[1].delete(&key);
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
            18 => {
                // Counter increment
                let target = &counters[rng.gen_range(0..counters.len())];
                let x = (rng.gen::<f64>() - 0.5) * 10.0;
                let _ = target.increment(x);
            }
            19 => {
                // Counter decrement
                let target = &counters[rng.gen_range(0..counters.len())];
                let x = (rng.gen::<f64>() - 0.5) * 10.0;
                let _ = target.decrement(x);
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

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }

    let iters = parse_usize(&args, "--iters", 100);
    if iters == 0 {
        usage();
    }

    let ops = parse_usize(&args, "--ops", 200);
    let commit_every = parse_usize(&args, "--commit-every", 20);
    let peers_n = parse_usize(&args, "--peers", 1).max(1);

    let seed = parse_u64(
        &args,
        "--seed",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );

    let out_dir = parse_out_dir(&args);
    std::fs::create_dir_all(&out_dir)?;

    let moon_bin = std::env::var("MOON_BIN").unwrap_or_else(|_| "moon".to_string());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string());
    anyhow::ensure!(
        bin_available(&moon_bin, &["version"]),
        "moon not available (set MOON_BIN)"
    );
    anyhow::ensure!(
        bin_available(&node_bin, &["--version"]),
        "node not available (set NODE_BIN)"
    );

    let cli_js = build_moon_cli_js(&moon_bin)?;

    let peer_ids: Vec<u64> = (1..=peers_n as u64).collect();

    for i in 0..iters {
        let case_seed = seed.wrapping_add(i as u64);

        let doc = LoroDoc::new();
        apply_random_ops(&doc, case_seed, ops, commit_every, &peer_ids)?;
        let snapshot = doc.export(ExportMode::Snapshot)?;

        let expected = doc.get_deep_value().to_json_value();
        let moon_json = run_export_deep_json(&node_bin, &cli_js, &snapshot)?;
        let got: Value = serde_json::from_str(&moon_json)?;

        if !json_value_eq(&expected, &got) {
            let case_dir = out_dir.join(format!("case-{case_seed}"));
            std::fs::create_dir_all(&case_dir)?;
            std::fs::write(case_dir.join("snapshot.blob"), &snapshot)?;
            write_json(&case_dir.join("expected.json"), &expected)?;
            write_json(&case_dir.join("moon.parsed.json"), &got)?;
            std::fs::write(case_dir.join("moon.raw.json"), &moon_json)?;
            let meta = serde_json::json!({
                "seed": case_seed,
                "ops": ops,
                "commit_every": commit_every,
                "peers": peer_ids,
            });
            write_json(&case_dir.join("meta.json"), &meta)?;

            anyhow::bail!(
                "deep-json mismatch (seed={case_seed}); artifacts written to {}",
                case_dir.display()
            );
        }

        if (i + 1) % 50 == 0 {
            eprintln!("ok: {}/{} (seed={case_seed})", i + 1, iters);
        }
    }

    eprintln!("ok: all {iters} iterations passed (base_seed={seed})");
    Ok(())
}
