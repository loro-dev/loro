use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{ExportMode, LoroDoc, ToJson};

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
    let expected = doc.get_deep_value().to_json_value();

    // Updates e2e (FastUpdates): Rust export -> Moon transcode -> Rust import.
    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let out_updates = run_transcode(&node_bin, &cli_js, &updates)?;
    let doc2 = LoroDoc::new();
    doc2.import(&out_updates).unwrap();
    assert_eq!(doc2.get_deep_value().to_json_value(), expected);

    // Snapshot e2e (FastSnapshot): Rust export -> Moon transcode -> Rust import.
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let out_snapshot = run_transcode(&node_bin, &cli_js, &snapshot)?;
    let doc3 = LoroDoc::new();
    doc3.import(&out_snapshot).unwrap();
    assert_eq!(doc3.get_deep_value().to_json_value(), expected);

    Ok(())
}
