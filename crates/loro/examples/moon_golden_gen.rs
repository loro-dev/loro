use std::borrow::Cow;
use std::path::{Path, PathBuf};

use loro::{
    ExpandType, ExportMode, LoroDoc, LoroValue, StyleConfig, StyleConfigMap, Timestamp, ToJson,
    TreeParentId, VersionVector,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn usage() -> ! {
    eprintln!(
        r#"moon_golden_gen (loro)

Generate a deterministic random Loro document and export:
- FastUpdates (binary) + JsonUpdates (JsonSchema)
- FastSnapshot (binary) + deep JSON (get_deep_value)

Usage:
  cargo run -p loro --example moon_golden_gen -- \
    --out-dir <dir> [--seed <u64>] [--ops <n>] [--commit-every <n>] [--peers <n>]

Outputs in <dir>:
  - updates.blob
  - updates.json
  - snapshot.blob
  - snapshot.deep.json
  - meta.json
"#
    );
    std::process::exit(2);
}

fn parse_arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|w| (w[0] == name).then_some(w[1].as_str()))
}

fn parse_u64(args: &[String], name: &str, default: u64) -> u64 {
    match parse_arg_value(args, name) {
        None => default,
        Some(v) => v.parse().unwrap_or_else(|_| usage()),
    }
}

fn parse_usize(args: &[String], name: &str, default: usize) -> usize {
    match parse_arg_value(args, name) {
        None => default,
        Some(v) => v.parse().unwrap_or_else(|_| usage()),
    }
}

fn parse_out_dir(args: &[String]) -> PathBuf {
    parse_arg_value(args, "--out-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| usage())
}

fn write_json(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    std::fs::write(path, s)?;
    Ok(())
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

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }

    let out_dir = parse_out_dir(&args);
    let seed = parse_u64(&args, "--seed", 1);
    let ops = parse_usize(&args, "--ops", 200);
    let commit_every = parse_usize(&args, "--commit-every", 20);
    let peers = parse_usize(&args, "--peers", 1);

    std::fs::create_dir_all(&out_dir)?;

    let doc = LoroDoc::new();
    let peer_ids: Vec<u64> = (1..=peers.max(1) as u64).collect();
    apply_random_ops(&doc, seed, ops, commit_every, &peer_ids)?;

    let start = VersionVector::default();
    let end = doc.oplog_vv();

    let updates_blob = doc.export(ExportMode::Updates {
        from: Cow::Borrowed(&start),
    })?;
    std::fs::write(out_dir.join("updates.blob"), &updates_blob)?;

    let updates_schema = doc.export_json_updates(&start, &end);
    let updates_json = serde_json::to_value(&updates_schema)?;
    write_json(&out_dir.join("updates.json"), &updates_json)?;

    let snapshot_blob = doc.export(ExportMode::Snapshot)?;
    std::fs::write(out_dir.join("snapshot.blob"), &snapshot_blob)?;

    let deep = doc.get_deep_value().to_json_value();
    write_json(&out_dir.join("snapshot.deep.json"), &deep)?;

    let meta = serde_json::json!({
        "seed": seed,
        "ops": ops,
        "commit_every": commit_every,
        "peers": peers,
    });
    write_json(&out_dir.join("meta.json"), &meta)?;

    Ok(())
}
