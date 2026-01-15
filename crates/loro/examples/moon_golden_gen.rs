use std::borrow::Cow;
use std::path::{Path, PathBuf};

use loro::{ExportMode, LoroDoc, LoroValue, Timestamp, ToJson, TreeParentId, VersionVector};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn usage() -> ! {
    eprintln!(
        r#"moon_golden_gen (loro)

Generate a deterministic random Loro document and export:
- FastUpdates (binary) + JsonUpdates (JsonSchema)
- FastSnapshot (binary) + deep JSON (get_deep_value)

Usage:
  cargo run -p loro --example moon_golden_gen -- \
    --out-dir <dir> [--seed <u64>] [--ops <n>] [--commit-every <n>]

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

fn apply_random_ops(doc: &LoroDoc, seed: u64, ops: usize, commit_every: usize) -> anyhow::Result<()> {
    let mut rng = StdRng::seed_from_u64(seed);

    doc.set_peer_id(1)?;
    let map = doc.get_map("map");
    let list = doc.get_list("list");
    let text = doc.get_text("text");
    let mlist = doc.get_movable_list("mlist");
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);

    let mut tree_nodes = Vec::new();

    for i in 0..ops {
        let op_type = rng.gen_range(0..11);
        match op_type {
            0 => {
                let key = format!("k{}", rng.gen::<u32>());
                let value = LoroValue::from(rng.gen::<i32>());
                map.insert(&key, value)?;
            }
            1 => {
                let key = format!("k{}", rng.gen::<u32>());
                let value = if rng.gen::<bool>() {
                    LoroValue::from(rng.gen::<bool>())
                } else {
                    LoroValue::Null
                };
                map.insert(&key, value)?;
            }
            2 => {
                if !map.is_empty() {
                    let key = format!("k{}", rng.gen::<u32>());
                    let _ = map.delete(&key);
                }
            }
            3 => {
                let index = rng.gen_range(0..=list.len());
                let value = LoroValue::from(rng.gen::<i32>());
                list.insert(index, value)?;
            }
            4 => {
                if !list.is_empty() {
                    let index = rng.gen_range(0..list.len());
                    list.delete(index, 1)?;
                }
            }
            5 => {
                let index = rng.gen_range(0..=text.len_unicode());
                let s = match rng.gen_range(0..6) {
                    0 => "a",
                    1 => "b",
                    2 => "Z",
                    3 => "😀",
                    4 => "中",
                    _ => "!",
                };
                text.insert(index, s)?;
            }
            6 => {
                if text.len_unicode() > 0 {
                    let index = rng.gen_range(0..text.len_unicode());
                    text.delete(index, 1)?;
                }
            }
            7 => {
                // MovableList: insert
                let index = rng.gen_range(0..=mlist.len());
                let value = LoroValue::from(rng.gen::<i32>());
                mlist.insert(index, value)?;
            }
            8 => {
                // MovableList: set
                if !mlist.is_empty() {
                    let index = rng.gen_range(0..mlist.len());
                    let value = LoroValue::from(rng.gen::<i32>());
                    mlist.set(index, value)?;
                }
            }
            9 => {
                // MovableList: move/delete
                if mlist.len() >= 2 && rng.gen::<bool>() {
                    let from = rng.gen_range(0..mlist.len());
                    let to = rng.gen_range(0..mlist.len());
                    let _ = mlist.mov(from, to);
                } else if !mlist.is_empty() {
                    let index = rng.gen_range(0..mlist.len());
                    mlist.delete(index, 1)?;
                }
            }
            10 => {
                // Tree: create/move/delete/meta
                match rng.gen_range(0..4) {
                    0 => {
                        let parent = if tree_nodes.is_empty() || rng.gen::<bool>() {
                            TreeParentId::Root
                        } else {
                            TreeParentId::from(tree_nodes[rng.gen_range(0..tree_nodes.len())])
                        };
                        let id = tree.create(parent)?;
                        tree_nodes.push(id);
                    }
                    1 => {
                        if tree_nodes.len() >= 2 {
                            let target = tree_nodes[rng.gen_range(0..tree_nodes.len())];
                            let parent = if rng.gen::<bool>() {
                                TreeParentId::Root
                            } else {
                                TreeParentId::from(tree_nodes[rng.gen_range(0..tree_nodes.len())])
                            };
                            let _ = tree.mov(target, parent);
                        }
                    }
                    2 => {
                        if !tree_nodes.is_empty() {
                            let idx = rng.gen_range(0..tree_nodes.len());
                            let id = tree_nodes.swap_remove(idx);
                            let _ = tree.delete(id);
                        }
                    }
                    _ => {
                        if !tree_nodes.is_empty() {
                            let id = tree_nodes[rng.gen_range(0..tree_nodes.len())];
                            let meta = tree.get_meta(id)?;
                            let key = format!("m{}", rng.gen::<u8>());
                            meta.insert(&key, rng.gen::<i32>())?;
                        }
                    }
                }
            }
            _ => unreachable!(),
        }

        if commit_every > 0 && (i + 1) % commit_every == 0 {
            let msg = format!("commit-{} seed={}", i + 1, seed);
            doc.set_next_commit_message(&msg);
            doc.set_next_commit_timestamp((i as i64) as Timestamp);
            doc.commit();
        }
    }

    let msg = format!("final seed={seed} ops={ops}");
    doc.set_next_commit_message(&msg);
    doc.set_next_commit_timestamp((ops as i64) as Timestamp);
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

    std::fs::create_dir_all(&out_dir)?;

    let doc = LoroDoc::new();
    apply_random_ops(&doc, seed, ops, commit_every)?;

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
    });
    write_json(&out_dir.join("meta.json"), &meta)?;

    Ok(())
}
