use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use loro::{
    Container, ContainerTrait, ExportMode, JsonSchema, LoroDoc, LoroList, LoroMap, LoroMovableList,
    LoroValue, ValueOrContainer,
};
use rustc_hash::FxHashSet;

#[derive(Debug, Clone)]
enum MirrorState {
    Null,
    Bool(bool),
    Double(f64),
    I64(i64),
    Binary(Vec<u8>),
    String(String),
    List(Vec<MirrorState>),
    Map {
        cid: Option<String>,
        entries: Vec<(String, MirrorState)>,
    },
    ContainerId(String),
}

impl MirrorState {
    #[inline(never)]
    fn touch(&self) -> usize {
        match self {
            MirrorState::Null => 1,
            MirrorState::Bool(v) => usize::from(*v),
            MirrorState::Double(v) => v.to_bits() as usize,
            MirrorState::I64(v) => *v as usize,
            MirrorState::Binary(v) => v.len(),
            MirrorState::String(v) => v.len(),
            MirrorState::List(items) => items.len(),
            MirrorState::Map { cid, entries } => {
                entries.len() + cid.as_ref().map_or(0, |x| x.len())
            }
            MirrorState::ContainerId(cid) => cid.len(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct StateStats {
    maps: usize,
    lists: usize,
    movable_lists: usize,
    texts: usize,
    trees: usize,
    counters: usize,
    unknown_containers: usize,
    map_entries: usize,
    list_items: usize,
    primitive_values: usize,
    string_bytes: usize,
    binary_bytes: usize,
    cid_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct ConstructTimings {
    first_snapshot: Duration,
    register_containers: Duration,
    second_snapshot: Duration,
}

#[derive(Debug)]
struct MirrorConstructResult {
    state: MirrorState,
    first_state_stats: StateStats,
    second_state_stats: StateStats,
    registry_stats: StateStats,
    timings: ConstructTimings,
    checksum: u64,
}

#[derive(Debug)]
struct Args {
    phase: String,
    input_json: Option<PathBuf>,
    snapshot: PathBuf,
    repeat: usize,
    warmup: usize,
    pprof_flamegraph: Option<PathBuf>,
    pprof_frequency: i32,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    match args.phase.as_str() {
        "prepare" => prepare_snapshot(&args),
        "prepare-state-only" => prepare_state_only_snapshot(&args),
        "once" => run_once(&args),
        "doc-deep-value" => run_doc_deep_value_loop(&args),
        "snapshot-state-only-value" => run_snapshot_state_only_value_loop(&args, false),
        "snapshot-state-only-value-with-id" => run_snapshot_state_only_value_loop(&args, true),
        "snapshot-state-only-mirror-value" => run_snapshot_state_only_mirror_value_loop(&args),
        "from-snapshot-and-mirror-value" => run_from_snapshot_and_mirror_value_loop(&args),
        "mirror-snapshot" => run_mirror_snapshot_loop(&args),
        "mirror-construct" => run_mirror_construct_loop(&args),
        "import-snapshot-and-read" => run_import_snapshot_and_read_loop(&args),
        "import-breakdown" => run_import_breakdown_loop(&args),
        "import-breakdown-batch" => run_import_breakdown_batch_loop(&args),
        "import-read-once" => run_import_read_once_loop(&args, false),
        "import-register-read-once" => run_import_read_once_loop(&args, true),
        "mirror-snapshot-batch" => run_mirror_snapshot_batch_loop(&args),
        "import-read-once-batch" => run_import_read_once_batch_loop(&args),
        phase => bail!(
            "unknown phase {phase:?}; use prepare, prepare-state-only, once, doc-deep-value, snapshot-state-only-value, snapshot-state-only-value-with-id, snapshot-state-only-mirror-value, from-snapshot-and-mirror-value, mirror-snapshot, mirror-snapshot-batch, mirror-construct, import-snapshot-and-read, import-breakdown, import-breakdown-batch, import-read-once, import-read-once-batch, or import-register-read-once"
        ),
    }
}

fn parse_args() -> Result<Args> {
    let mut args = env::args().skip(1);
    let mut parsed = Args {
        phase: "once".to_string(),
        input_json: None,
        snapshot: PathBuf::from("/tmp/loro-profile-input/tmp-doc.snapshot"),
        repeat: 1,
        warmup: 0,
        pprof_flamegraph: None,
        pprof_frequency: 997,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--phase" => parsed.phase = next_arg(&mut args, "--phase")?,
            "--input-json" => parsed.input_json = Some(next_arg(&mut args, "--input-json")?.into()),
            "--snapshot" => parsed.snapshot = next_arg(&mut args, "--snapshot")?.into(),
            "--repeat" => {
                parsed.repeat = next_arg(&mut args, "--repeat")?
                    .parse()
                    .context("failed to parse --repeat")?
            }
            "--warmup" => {
                parsed.warmup = next_arg(&mut args, "--warmup")?
                    .parse()
                    .context("failed to parse --warmup")?
            }
            "--pprof-flamegraph" => {
                parsed.pprof_flamegraph = Some(next_arg(&mut args, "--pprof-flamegraph")?.into())
            }
            "--pprof-frequency" => {
                parsed.pprof_frequency = next_arg(&mut args, "--pprof-frequency")?
                    .parse()
                    .context("failed to parse --pprof-frequency")?
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => bail!("unknown arg {arg:?}; pass --help to see usage"),
        }
    }

    if parsed.repeat == 0 {
        bail!("--repeat must be greater than zero");
    }

    Ok(parsed)
}

fn next_arg(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String> {
    args.next()
        .with_context(|| format!("missing value after {name}"))
}

fn print_help() {
    println!(
        "Usage:
  cargo run -p loro --release --example profile_mirror_read -- \\
    --phase prepare --input-json /path/to/tmp-doc.json --snapshot /tmp/tmp-doc.snapshot

  cargo run -p loro --release --example profile_mirror_read -- \\
    --phase once --snapshot /tmp/tmp-doc.snapshot

  cargo run -p loro --release --example profile_mirror_read -- \\
    --phase mirror-construct --snapshot /tmp/tmp-doc.snapshot --repeat 100

  cargo run -p loro --release --example profile_mirror_read -- \\
    --phase mirror-construct --snapshot /tmp/tmp-doc.snapshot --repeat 100 \\
    --pprof-flamegraph /tmp/mirror-construct.svg

Phases:
  prepare                   import JSON schema updates and export a snapshot
  prepare-state-only        import JSON schema updates and export a state-only snapshot
  once                      time one snapshot import plus mirror-style constructor read
  doc-deep-value            repeatedly read LoroDoc::get_deep_value as a baseline
  snapshot-state-only-value repeatedly decode snapshot state bytes directly to LoroValue
  snapshot-state-only-value-with-id
                            same as snapshot-state-only-value, with container IDs
  snapshot-state-only-mirror-value
                            same as snapshot-state-only-value, with map $cid fields
  from-snapshot-and-mirror-value
                            import snapshot, then read mirror-shaped value once
  mirror-snapshot           repeatedly build the root state snapshot from one imported doc
  mirror-snapshot-batch     same as mirror-snapshot, but uses for_each/entries-style reads
  mirror-construct          repeatedly simulate the schema Mirror constructor read path
  import-snapshot-and-read  repeatedly import snapshot and simulate Mirror constructor read
  import-breakdown          repeatedly import snapshot and print per-stage averages
  import-breakdown-batch    same as import-breakdown, but uses entries-style reads and the
                            actual registry scan shape that does not read text contents
  import-read-once          import snapshot and build root state once
  import-read-once-batch    import snapshot and build root state once with entries-style reads
  import-register-read-once import snapshot, registry scan, and build root state once

Profiling:
  --pprof-flamegraph PATH   write a userspace sampling flamegraph SVG
  --pprof-frequency N       sampling frequency for --pprof-flamegraph, default 997"
    );
}

#[inline(never)]
fn prepare_snapshot(args: &Args) -> Result<()> {
    prepare_snapshot_with_mode(args, ExportMode::Snapshot, "export_snapshot_ms")
}

#[inline(never)]
fn prepare_state_only_snapshot(args: &Args) -> Result<()> {
    prepare_snapshot_with_mode(args, ExportMode::StateOnly(None), "export_state_only_ms")
}

#[inline(never)]
fn prepare_snapshot_with_mode(
    args: &Args,
    export_mode: ExportMode<'_>,
    export_label: &str,
) -> Result<()> {
    let input_json = args
        .input_json
        .as_ref()
        .context("--input-json is required for --phase prepare")?;

    let start = Instant::now();
    let bytes = fs::read(input_json)
        .with_context(|| format!("failed to read input JSON {}", input_json.display()))?;
    let read_json = start.elapsed();

    let start = Instant::now();
    let json: JsonSchema = serde_json::from_slice(&bytes).context("failed to parse JsonSchema")?;
    let parse_json = start.elapsed();

    let doc = LoroDoc::new();
    let start = Instant::now();
    let status = doc
        .import_json_updates(json)
        .context("failed to import JSON schema updates")?;
    let import_json = start.elapsed();
    if status.pending.is_some() {
        bail!(
            "JSON import left pending dependencies: {:?}",
            status.pending
        );
    }

    let start = Instant::now();
    let snapshot = doc
        .export(export_mode)
        .context("failed to export snapshot")?;
    let export_snapshot = start.elapsed();

    if let Some(parent) = args.snapshot.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let start = Instant::now();
    fs::write(&args.snapshot, &snapshot)
        .with_context(|| format!("failed to write {}", args.snapshot.display()))?;
    let write_snapshot = start.elapsed();

    println!("input_json_bytes={}", bytes.len());
    println!("snapshot_bytes={}", snapshot.len());
    println!("read_json_ms={:.3}", ms(read_json));
    println!("parse_json_ms={:.3}", ms(parse_json));
    println!("import_json_updates_ms={:.3}", ms(import_json));
    println!("{export_label}={:.3}", ms(export_snapshot));
    println!("write_snapshot_ms={:.3}", ms(write_snapshot));
    println!("snapshot_path={}", args.snapshot.display());

    Ok(())
}

#[inline(never)]
fn run_once(args: &Args) -> Result<()> {
    let start = Instant::now();
    let snapshot = read_snapshot(&args.snapshot)?;
    let read_snapshot = start.elapsed();

    let start = Instant::now();
    let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
    let import_snapshot = start.elapsed();

    let result = mirror_construct(&doc);
    black_box(&result.state);

    println!("snapshot_bytes={}", snapshot.len());
    println!("read_snapshot_ms={:.3}", ms(read_snapshot));
    println!("import_snapshot_ms={:.3}", ms(import_snapshot));
    println!(
        "mirror_first_build_root_state_snapshot_ms={:.3}",
        ms(result.timings.first_snapshot)
    );
    println!(
        "mirror_register_nested_containers_ms={:.3}",
        ms(result.timings.register_containers)
    );
    println!(
        "mirror_second_build_root_state_snapshot_ms={:.3}",
        ms(result.timings.second_snapshot)
    );
    println!(
        "mirror_constructor_read_total_ms={:.3}",
        ms(result.timings.first_snapshot
            + result.timings.register_containers
            + result.timings.second_snapshot)
    );
    println!("checksum={}", result.checksum);
    print_stats("first_state", result.first_state_stats);
    print_stats("registry_scan", result.registry_stats);
    print_stats("second_state", result.second_state_stats);

    Ok(())
}

#[inline(never)]
fn run_doc_deep_value_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;
    let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;

    for _ in 0..args.warmup {
        let value = doc.get_deep_value();
        black_box(value);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0usize;
        let start = Instant::now();
        for iter in 0..args.repeat {
            let value = doc.get_deep_value();
            checksum ^= black_box(iter);
            black_box(value);
        }
        let elapsed = start.elapsed();

        println!("phase=doc-deep-value");
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

#[inline(never)]
fn run_snapshot_state_only_value_loop(args: &Args, include_container_id: bool) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let value = if include_container_id {
            LoroDoc::decode_snapshot_state_only_value_with_id(&snapshot)
        } else {
            LoroDoc::decode_snapshot_state_only_value(&snapshot)
        }
        .context("failed to decode snapshot state value")?;
        black_box(value);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0usize;
        let start = Instant::now();
        for iter in 0..args.repeat {
            let value = if include_container_id {
                LoroDoc::decode_snapshot_state_only_value_with_id(&snapshot)
            } else {
                LoroDoc::decode_snapshot_state_only_value(&snapshot)
            }
            .context("failed to decode snapshot state value")?;
            checksum ^= black_box(iter);
            black_box(value);
        }
        let elapsed = start.elapsed();

        println!(
            "phase={}",
            if include_container_id {
                "snapshot-state-only-value-with-id"
            } else {
                "snapshot-state-only-value"
            }
        );
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

#[inline(never)]
fn run_snapshot_state_only_mirror_value_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let value = LoroDoc::decode_snapshot_state_only_mirror_value(&snapshot)
            .context("failed to decode snapshot mirror value")?;
        black_box(value);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0usize;
        let start = Instant::now();
        for iter in 0..args.repeat {
            let value = LoroDoc::decode_snapshot_state_only_mirror_value(&snapshot)
                .context("failed to decode snapshot mirror value")?;
            checksum ^= black_box(iter);
            black_box(value);
        }
        let elapsed = start.elapsed();

        println!("phase=snapshot-state-only-mirror-value");
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

#[inline(never)]
fn run_from_snapshot_and_mirror_value_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        let value = doc.get_deep_value_with_map_id();
        black_box(value);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0usize;
        let start = Instant::now();
        for iter in 0..args.repeat {
            let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
            let value = doc.get_deep_value_with_map_id();
            checksum ^= black_box(iter);
            black_box(value);
        }
        let elapsed = start.elapsed();

        println!("phase=from-snapshot-and-mirror-value");
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

#[inline(never)]
fn run_mirror_snapshot_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;
    let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;

    for _ in 0..args.warmup {
        let (state, stats) = build_root_state_snapshot(&doc);
        black_box(state.touch());
        black_box(state);
        black_box(stats);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0;
        let start = Instant::now();
        for _ in 0..args.repeat {
            let (state, stats) = build_root_state_snapshot(&doc);
            checksum ^= black_box(stats_digest(stats));
            black_box(state);
            black_box(stats);
        }
        let elapsed = start.elapsed();

        println!("phase=mirror-snapshot");
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

#[inline(never)]
fn run_mirror_snapshot_batch_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;
    let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;

    for _ in 0..args.warmup {
        let (state, stats) = build_root_state_snapshot_batch(&doc);
        black_box(state.touch());
        black_box(state);
        black_box(stats);
    }

    let mut checksum = 0;
    let start = Instant::now();
    for _ in 0..args.repeat {
        let (state, stats) = build_root_state_snapshot_batch(&doc);
        checksum ^= black_box(stats_digest(stats));
        black_box(state.touch());
        black_box(state);
    }
    let elapsed = start.elapsed();

    println!("phase=mirror-snapshot-batch");
    println!("repeat={}", args.repeat);
    println!("elapsed_ms={:.3}", ms(elapsed));
    println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
    println!("checksum={checksum}");

    Ok(())
}

#[inline(never)]
fn run_mirror_construct_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;
    let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;

    for _ in 0..args.warmup {
        let result = mirror_construct(&doc);
        black_box(result.checksum);
        black_box(result.state);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0;
        let start = Instant::now();
        for _ in 0..args.repeat {
            let result = mirror_construct(&doc);
            checksum ^= black_box(result.checksum);
            black_box(result.state);
        }
        let elapsed = start.elapsed();

        println!("phase=mirror-construct");
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

#[derive(Default)]
struct BreakdownTimings {
    import_snapshot: Duration,
    first_snapshot: Duration,
    register_containers: Duration,
    second_snapshot: Duration,
    drop_doc: Duration,
    total: Duration,
}

#[inline(never)]
fn run_import_breakdown_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        let result = mirror_construct(&doc);
        black_box(result.checksum);
        black_box(result.state);
    }

    let mut timings = BreakdownTimings::default();
    let mut checksum = 0;
    for _ in 0..args.repeat {
        let iter_start = Instant::now();

        let start = Instant::now();
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        timings.import_snapshot += start.elapsed();

        let start = Instant::now();
        let (first_state, first_stats) = build_root_state_snapshot(&doc);
        timings.first_snapshot += start.elapsed();
        black_box(first_state.touch());
        black_box(first_state);
        checksum ^= black_box(stats_digest(first_stats));

        let start = Instant::now();
        let registry_stats = register_schema_containers(&doc);
        timings.register_containers += start.elapsed();
        checksum ^= black_box(stats_digest(registry_stats).rotate_left(17));

        let start = Instant::now();
        let (second_state, second_stats) = build_root_state_snapshot(&doc);
        timings.second_snapshot += start.elapsed();
        black_box(second_state.touch());
        black_box(second_state);
        checksum ^= black_box(stats_digest(second_stats).rotate_left(31));

        let start = Instant::now();
        drop(doc);
        timings.drop_doc += start.elapsed();
        timings.total += iter_start.elapsed();
    }

    println!("phase=import-breakdown");
    println!("repeat={}", args.repeat);
    print_avg("import_snapshot", timings.import_snapshot, args.repeat);
    print_avg(
        "first_build_root_state_snapshot",
        timings.first_snapshot,
        args.repeat,
    );
    print_avg(
        "register_nested_containers",
        timings.register_containers,
        args.repeat,
    );
    print_avg(
        "second_build_root_state_snapshot",
        timings.second_snapshot,
        args.repeat,
    );
    print_avg("drop_doc", timings.drop_doc, args.repeat);
    print_avg("total", timings.total, args.repeat);
    println!("checksum={checksum}");

    Ok(())
}

#[inline(never)]
fn run_import_breakdown_batch_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        let (first_state, first_stats) = build_root_state_snapshot_batch(&doc);
        black_box(first_state.touch());
        black_box(first_state);
        black_box(first_stats);
        black_box(register_schema_containers_batch(&doc));
        let (second_state, second_stats) = build_root_state_snapshot_batch(&doc);
        black_box(second_state.touch());
        black_box(second_state);
        black_box(second_stats);
    }

    let mut timings = BreakdownTimings::default();
    let mut checksum = 0;
    for _ in 0..args.repeat {
        let iter_start = Instant::now();

        let start = Instant::now();
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        timings.import_snapshot += start.elapsed();

        let start = Instant::now();
        let (first_state, first_stats) = build_root_state_snapshot_batch(&doc);
        timings.first_snapshot += start.elapsed();
        black_box(first_state.touch());
        black_box(first_state);
        checksum ^= black_box(stats_digest(first_stats));

        let start = Instant::now();
        let registry_stats = register_schema_containers_batch(&doc);
        timings.register_containers += start.elapsed();
        checksum ^= black_box(stats_digest(registry_stats).rotate_left(17));

        let start = Instant::now();
        let (second_state, second_stats) = build_root_state_snapshot_batch(&doc);
        timings.second_snapshot += start.elapsed();
        black_box(second_state.touch());
        black_box(second_state);
        checksum ^= black_box(stats_digest(second_stats).rotate_left(31));

        let start = Instant::now();
        drop(doc);
        timings.drop_doc += start.elapsed();
        timings.total += iter_start.elapsed();
    }

    println!("phase=import-breakdown-batch");
    println!("repeat={}", args.repeat);
    print_avg("import_snapshot", timings.import_snapshot, args.repeat);
    print_avg(
        "first_build_root_state_snapshot",
        timings.first_snapshot,
        args.repeat,
    );
    print_avg(
        "register_nested_containers",
        timings.register_containers,
        args.repeat,
    );
    print_avg(
        "second_build_root_state_snapshot",
        timings.second_snapshot,
        args.repeat,
    );
    print_avg("drop_doc", timings.drop_doc, args.repeat);
    print_avg("total", timings.total, args.repeat);
    println!("checksum={checksum}");

    Ok(())
}

#[inline(never)]
fn run_import_read_once_loop(args: &Args, with_registry: bool) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        if with_registry {
            black_box(register_schema_containers(&doc));
        }
        let (state, stats) = build_root_state_snapshot(&doc);
        black_box(state.touch());
        black_box(state);
        black_box(stats);
    }

    let mut checksum = 0;
    let start = Instant::now();
    for _ in 0..args.repeat {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        if with_registry {
            let stats = register_schema_containers(&doc);
            checksum ^= black_box(stats_digest(stats).rotate_left(17));
        }
        let (state, stats) = build_root_state_snapshot(&doc);
        checksum ^= black_box(stats_digest(stats));
        black_box(state.touch());
        black_box(state);
    }
    let elapsed = start.elapsed();

    println!(
        "phase={}",
        if with_registry {
            "import-register-read-once"
        } else {
            "import-read-once"
        }
    );
    println!("repeat={}", args.repeat);
    println!("elapsed_ms={:.3}", ms(elapsed));
    println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
    println!("checksum={checksum}");

    Ok(())
}

#[inline(never)]
fn run_import_read_once_batch_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        let (state, stats) = build_root_state_snapshot_batch(&doc);
        black_box(state.touch());
        black_box(state);
        black_box(stats);
    }

    let mut checksum = 0;
    let start = Instant::now();
    for _ in 0..args.repeat {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        let (state, stats) = build_root_state_snapshot_batch(&doc);
        checksum ^= black_box(stats_digest(stats));
        black_box(state.touch());
        black_box(state);
    }
    let elapsed = start.elapsed();

    println!("phase=import-read-once-batch");
    println!("repeat={}", args.repeat);
    println!("elapsed_ms={:.3}", ms(elapsed));
    println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
    println!("checksum={checksum}");

    Ok(())
}

#[inline(never)]
fn run_import_snapshot_and_read_loop(args: &Args) -> Result<()> {
    let snapshot = read_snapshot(&args.snapshot)?;

    for _ in 0..args.warmup {
        let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
        let result = mirror_construct(&doc);
        black_box(result.checksum);
        black_box(result.state);
    }

    run_with_optional_pprof(args, || {
        let mut checksum = 0;
        let start = Instant::now();
        for _ in 0..args.repeat {
            let doc = LoroDoc::from_snapshot(&snapshot).context("failed to import snapshot")?;
            let result = mirror_construct(&doc);
            checksum ^= black_box(result.checksum);
            black_box(result.state);
        }
        let elapsed = start.elapsed();

        println!("phase=import-snapshot-and-read");
        println!("repeat={}", args.repeat);
        println!("elapsed_ms={:.3}", ms(elapsed));
        println!("per_iter_ms={:.3}", ms(elapsed) / args.repeat as f64);
        println!("checksum={checksum}");
        Ok(())
    })
}

fn run_with_optional_pprof<F>(args: &Args, f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    let Some(path) = args.pprof_flamegraph.as_ref() else {
        return f();
    };

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(args.pprof_frequency)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .map_err(|err| anyhow!("failed to start pprof profiler: {err:?}"))?;

    let result = f();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let report = guard
        .report()
        .build()
        .map_err(|err| anyhow!("failed to build pprof report: {err:?}"))?;
    let file = fs::File::create(path)
        .with_context(|| format!("failed to create flamegraph {}", path.display()))?;
    report
        .flamegraph(file)
        .map_err(|err| anyhow!("failed to write flamegraph {}: {err:?}", path.display()))?;
    println!("pprof_flamegraph={}", path.display());

    result
}

fn read_snapshot(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("failed to read snapshot {}", path.display()))
}

#[inline(never)]
fn mirror_construct(doc: &LoroDoc) -> MirrorConstructResult {
    // This mirrors the schema-backed constructor path in loro-mirror:
    // 1. buildRootStateSnapshot overlays current doc state on schema defaults
    // 2. initializeContainers/registerNestedContainers scans every nested container
    // 3. buildRootStateSnapshot runs again and replaces baseState
    let start = Instant::now();
    let (first_state, first_state_stats) = build_root_state_snapshot(doc);
    let first_snapshot = start.elapsed();
    black_box(first_state.touch());
    black_box(first_state);

    let start = Instant::now();
    let registry_stats = register_schema_containers(doc);
    let register_containers = start.elapsed();

    let start = Instant::now();
    let (state, second_state_stats) = build_root_state_snapshot(doc);
    let second_snapshot = start.elapsed();
    let checksum = stats_digest(first_state_stats)
        ^ stats_digest(registry_stats).rotate_left(17)
        ^ stats_digest(second_state_stats).rotate_left(31);

    MirrorConstructResult {
        state,
        first_state_stats,
        second_state_stats,
        registry_stats,
        timings: ConstructTimings {
            first_snapshot,
            register_containers,
            second_snapshot,
        },
        checksum,
    }
}

#[inline(never)]
fn build_root_state_snapshot(doc: &LoroDoc) -> (MirrorState, StateStats) {
    let mut stats = StateStats::default();
    let mut entries = Vec::with_capacity(2);

    if let Some(history) = doc.try_get_list("history") {
        let state = read_list(&history, &mut stats);
        entries.push(("history".to_string(), state));
    }

    if let Some(mq) = doc.try_get_movable_list("mq") {
        let state = read_movable_list(&mq, &mut stats);
        entries.push(("mq".to_string(), state));
    }

    (MirrorState::Map { cid: None, entries }, stats)
}

#[inline(never)]
fn build_root_state_snapshot_batch(doc: &LoroDoc) -> (MirrorState, StateStats) {
    let mut stats = StateStats::default();
    let mut entries = Vec::with_capacity(2);

    if let Some(history) = doc.try_get_list("history") {
        let state = read_list_batch(&history, &mut stats);
        entries.push(("history".to_string(), state));
    }

    if let Some(mq) = doc.try_get_movable_list("mq") {
        let state = read_movable_list_batch(&mq, &mut stats);
        entries.push(("mq".to_string(), state));
    }

    (MirrorState::Map { cid: None, entries }, stats)
}

#[inline(never)]
fn read_container(container: &Container, stats: &mut StateStats) -> MirrorState {
    match container {
        Container::List(list) => read_list(list, stats),
        Container::Map(map) => read_map(map, stats),
        Container::Text(text) => {
            stats.texts += 1;
            let value = text.to_string();
            stats.string_bytes += value.len();
            MirrorState::String(value)
        }
        Container::Tree(tree) => {
            stats.trees += 1;
            value_to_state(tree.get_value(), stats)
        }
        Container::MovableList(list) => read_movable_list(list, stats),
        #[cfg(feature = "counter")]
        Container::Counter(counter) => {
            stats.counters += 1;
            MirrorState::Double(counter.get_value())
        }
        Container::Unknown(unknown) => {
            stats.unknown_containers += 1;
            MirrorState::ContainerId(unknown.id().to_string_fast())
        }
    }
}

#[inline(never)]
fn read_container_batch(container: &Container, stats: &mut StateStats) -> MirrorState {
    match container {
        Container::List(list) => read_list_batch(list, stats),
        Container::Map(map) => read_map_batch(map, stats),
        Container::Text(text) => {
            stats.texts += 1;
            let value = text.to_string();
            stats.string_bytes += value.len();
            MirrorState::String(value)
        }
        Container::Tree(tree) => {
            stats.trees += 1;
            value_to_state(tree.get_value(), stats)
        }
        Container::MovableList(list) => read_movable_list_batch(list, stats),
        #[cfg(feature = "counter")]
        Container::Counter(counter) => {
            stats.counters += 1;
            MirrorState::Double(counter.get_value())
        }
        Container::Unknown(unknown) => {
            stats.unknown_containers += 1;
            MirrorState::ContainerId(unknown.id().to_string_fast())
        }
    }
}

#[inline(never)]
fn read_map(map: &LoroMap, stats: &mut StateStats) -> MirrorState {
    stats.maps += 1;
    let cid = map.id().to_string_fast();
    stats.cid_bytes += cid.len();
    let mut entries = Vec::new();

    for key in map.keys() {
        let key_str = key.as_str();
        let value = map
            .get(key_str)
            .expect("map.keys() returned a key that map.get() could not read");
        stats.map_entries += 1;
        entries.push((
            key_str.to_string(),
            value_or_container_to_state(value, stats),
        ));
    }

    MirrorState::Map {
        cid: Some(cid),
        entries,
    }
}

#[inline(never)]
fn read_map_batch(map: &LoroMap, stats: &mut StateStats) -> MirrorState {
    stats.maps += 1;
    let cid = map.id().to_string_fast();
    stats.cid_bytes += cid.len();
    let mut entries = Vec::new();

    map.for_each(|key, value| {
        stats.map_entries += 1;
        entries.push((
            key.to_string(),
            value_or_container_to_state_batch(value, stats),
        ));
    });

    MirrorState::Map {
        cid: Some(cid),
        entries,
    }
}

#[inline(never)]
fn read_list(list: &LoroList, stats: &mut StateStats) -> MirrorState {
    stats.lists += 1;
    let len = list.len();
    stats.list_items += len;
    let mut items = Vec::with_capacity(len);

    for index in 0..len {
        let value = list
            .get(index)
            .expect("list.len() exposed an index that list.get() could not read");
        items.push(value_or_container_to_state(value, stats));
    }

    MirrorState::List(items)
}

#[inline(never)]
fn read_list_batch(list: &LoroList, stats: &mut StateStats) -> MirrorState {
    stats.lists += 1;
    let len = list.len();
    stats.list_items += len;
    let mut items = Vec::with_capacity(len);

    list.for_each(|value| {
        items.push(value_or_container_to_state_batch(value, stats));
    });

    MirrorState::List(items)
}

#[inline(never)]
fn read_movable_list(list: &LoroMovableList, stats: &mut StateStats) -> MirrorState {
    stats.movable_lists += 1;
    let len = list.len();
    stats.list_items += len;
    let mut items = Vec::with_capacity(len);

    for index in 0..len {
        let value = list
            .get(index)
            .expect("movable_list.len() exposed an index that movable_list.get() could not read");
        items.push(value_or_container_to_state(value, stats));
    }

    MirrorState::List(items)
}

#[inline(never)]
fn read_movable_list_batch(list: &LoroMovableList, stats: &mut StateStats) -> MirrorState {
    stats.movable_lists += 1;
    let len = list.len();
    stats.list_items += len;
    let mut items = Vec::with_capacity(len);

    list.for_each(|value| {
        items.push(value_or_container_to_state_batch(value, stats));
    });

    MirrorState::List(items)
}

#[inline(never)]
fn value_or_container_to_state(value: ValueOrContainer, stats: &mut StateStats) -> MirrorState {
    match value {
        ValueOrContainer::Value(value) => value_to_state(value, stats),
        ValueOrContainer::Container(container) => read_container(&container, stats),
    }
}

#[inline(never)]
fn value_or_container_to_state_batch(
    value: ValueOrContainer,
    stats: &mut StateStats,
) -> MirrorState {
    match value {
        ValueOrContainer::Value(value) => value_to_state(value, stats),
        ValueOrContainer::Container(container) => read_container_batch(&container, stats),
    }
}

#[inline(never)]
fn value_to_state(value: LoroValue, stats: &mut StateStats) -> MirrorState {
    stats.primitive_values += 1;
    match value {
        LoroValue::Null => MirrorState::Null,
        LoroValue::Bool(value) => MirrorState::Bool(value),
        LoroValue::Double(value) => MirrorState::Double(value),
        LoroValue::I64(value) => MirrorState::I64(value),
        LoroValue::Binary(value) => {
            stats.binary_bytes += value.len();
            MirrorState::Binary(value.to_vec())
        }
        LoroValue::String(value) => {
            stats.string_bytes += value.len();
            MirrorState::String(value.to_string())
        }
        LoroValue::List(value) => {
            let mut items = Vec::with_capacity(value.len());
            for item in value.iter() {
                items.push(value_to_state(item.clone(), stats));
            }
            MirrorState::List(items)
        }
        LoroValue::Map(value) => {
            let mut entries = Vec::with_capacity(value.len());
            for (key, item) in value.iter() {
                entries.push((key.clone(), value_to_state(item.clone(), stats)));
            }
            MirrorState::Map { cid: None, entries }
        }
        LoroValue::Container(cid) => MirrorState::ContainerId(cid.to_string_fast()),
    }
}

#[inline(never)]
fn register_schema_containers(doc: &LoroDoc) -> StateStats {
    let mut visited = FxHashSet::default();
    let mut stats = StateStats::default();

    if let Some(history) = doc.try_get_list("history") {
        register_container(&Container::List(history), &mut visited, &mut stats);
    }

    if let Some(mq) = doc.try_get_movable_list("mq") {
        register_container(&Container::MovableList(mq), &mut visited, &mut stats);
    }

    stats
}

#[inline(never)]
fn register_schema_containers_batch(doc: &LoroDoc) -> StateStats {
    let mut visited = FxHashSet::default();
    let mut stats = StateStats::default();

    if let Some(history) = doc.try_get_list("history") {
        register_container_batch(&Container::List(history), &mut visited, &mut stats);
    }

    if let Some(mq) = doc.try_get_movable_list("mq") {
        register_container_batch(&Container::MovableList(mq), &mut visited, &mut stats);
    }

    stats
}

#[inline(never)]
fn register_container(
    container: &Container,
    visited: &mut FxHashSet<String>,
    stats: &mut StateStats,
) {
    let cid = container.id().to_string_fast();
    let cid_len = cid.len();
    if !visited.insert(cid) {
        return;
    }
    stats.cid_bytes += cid_len;

    match container {
        Container::List(list) => {
            stats.lists += 1;
            let len = list.len();
            stats.list_items += len;
            for index in 0..len {
                if let Some(ValueOrContainer::Container(child)) = list.get(index) {
                    register_container(&child, visited, stats);
                }
            }
        }
        Container::Map(map) => {
            stats.maps += 1;
            for key in map.keys() {
                let key_str = key.as_str();
                stats.map_entries += 1;
                if let Some(ValueOrContainer::Container(child)) = map.get(key_str) {
                    register_container(&child, visited, stats);
                }
            }
        }
        Container::Text(text) => {
            stats.texts += 1;
            stats.string_bytes += text.len_utf8();
        }
        Container::Tree(tree) => {
            stats.trees += 1;
            let value = tree.get_value();
            let _ = black_box(value);
        }
        Container::MovableList(list) => {
            stats.movable_lists += 1;
            let len = list.len();
            stats.list_items += len;
            for index in 0..len {
                if let Some(ValueOrContainer::Container(child)) = list.get(index) {
                    register_container(&child, visited, stats);
                }
            }
        }
        #[cfg(feature = "counter")]
        Container::Counter(counter) => {
            stats.counters += 1;
            let _ = black_box(counter.get_value());
        }
        Container::Unknown(_) => {
            stats.unknown_containers += 1;
        }
    }
}

#[inline(never)]
fn register_container_batch(
    container: &Container,
    visited: &mut FxHashSet<String>,
    stats: &mut StateStats,
) {
    let cid = container.id().to_string_fast();
    let cid_len = cid.len();
    if !visited.insert(cid) {
        return;
    }
    stats.cid_bytes += cid_len;

    match container {
        Container::List(list) => {
            stats.lists += 1;
            let len = list.len();
            stats.list_items += len;
            list.for_each(|value| {
                if let ValueOrContainer::Container(child) = value {
                    register_container_batch(&child, visited, stats);
                }
            });
        }
        Container::Map(map) => {
            stats.maps += 1;
            map.for_each(|_, value| {
                stats.map_entries += 1;
                if let ValueOrContainer::Container(child) = value {
                    register_container_batch(&child, visited, stats);
                }
            });
        }
        Container::Text(_) => {
            stats.texts += 1;
        }
        Container::Tree(tree) => {
            stats.trees += 1;
            let value = tree.get_value();
            let _ = black_box(value);
        }
        Container::MovableList(list) => {
            stats.movable_lists += 1;
            let len = list.len();
            stats.list_items += len;
            list.for_each(|value| {
                if let ValueOrContainer::Container(child) = value {
                    register_container_batch(&child, visited, stats);
                }
            });
        }
        #[cfg(feature = "counter")]
        Container::Counter(counter) => {
            stats.counters += 1;
            let _ = black_box(counter.get_value());
        }
        Container::Unknown(_) => {
            stats.unknown_containers += 1;
        }
    }
}

fn print_stats(label: &str, stats: StateStats) {
    println!("{label}_maps={}", stats.maps);
    println!("{label}_lists={}", stats.lists);
    println!("{label}_movable_lists={}", stats.movable_lists);
    println!("{label}_texts={}", stats.texts);
    println!("{label}_trees={}", stats.trees);
    println!("{label}_counters={}", stats.counters);
    println!("{label}_unknown_containers={}", stats.unknown_containers);
    println!("{label}_map_entries={}", stats.map_entries);
    println!("{label}_list_items={}", stats.list_items);
    println!("{label}_primitive_values={}", stats.primitive_values);
    println!("{label}_string_bytes={}", stats.string_bytes);
    println!("{label}_binary_bytes={}", stats.binary_bytes);
    println!("{label}_cid_bytes={}", stats.cid_bytes);
}

fn print_avg(label: &str, duration: Duration, repeat: usize) {
    println!("{label}_avg_ms={:.3}", ms(duration) / repeat as f64);
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn stats_digest(stats: StateStats) -> u64 {
    let values = [
        stats.maps,
        stats.lists,
        stats.movable_lists,
        stats.texts,
        stats.trees,
        stats.counters,
        stats.unknown_containers,
        stats.map_entries,
        stats.list_items,
        stats.primitive_values,
        stats.string_bytes,
        stats.binary_bytes,
        stats.cid_bytes,
    ];
    let mut acc = 0xcbf2_9ce4_8422_2325u64;
    for value in values {
        acc = acc.rotate_left(5) ^ (value as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    acc
}
