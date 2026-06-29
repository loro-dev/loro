use std::{env, process, time::Duration};

use fuzz::crdt_fuzzer::{run_long_peer_fuzz, FuzzTarget, LongPeerFuzzConfig};

fn main() {
    let config = parse_args().unwrap_or_else(|err| {
        eprintln!("{err}");
        eprintln!();
        print_usage();
        process::exit(2);
    });

    eprintln!(
        "long_peer_fuzz start: seed={} peers={} max_ops={:?} duration={:?} sync_barrier_every={} check_every={} artifact_dir={} minimize={} minimize_time={:?}",
        config.seed,
        config.site_num,
        config.max_ops,
        config.duration,
        config.sync_barrier_every,
        config.check_every,
        config.artifact_dir.display(),
        config.minimize_on_failure,
        config.minimize_time
    );

    let stats = run_long_peer_fuzz(config);
    eprintln!(
        "long_peer_fuzz ok: ops={} elapsed={:.2}s",
        stats.ops,
        stats.elapsed.as_secs_f64()
    );
}

fn parse_args() -> Result<LongPeerFuzzConfig, String> {
    let mut config = LongPeerFuzzConfig::default();
    let mut saw_ops = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "--seed" => {
                config.seed = parse_next(&mut args, "--seed")?;
            }
            "--peers" => {
                config.site_num = parse_next(&mut args, "--peers")?;
            }
            "--ops" => {
                let ops: u64 = parse_next(&mut args, "--ops")?;
                config.max_ops = (ops != 0).then_some(ops);
                saw_ops = true;
            }
            "--duration-secs" => {
                let secs: u64 = parse_next(&mut args, "--duration-secs")?;
                config.duration = Some(Duration::from_secs(secs));
                if !saw_ops {
                    config.max_ops = None;
                }
            }
            "--sync-barrier-every" => {
                config.sync_barrier_every = parse_next(&mut args, "--sync-barrier-every")?;
            }
            "--check-every" => {
                config.check_every = parse_next(&mut args, "--check-every")?;
            }
            "--recent-actions" => {
                config.recent_actions = parse_next(&mut args, "--recent-actions")?;
            }
            "--nested-containers" => {
                config.include_nested_containers = true;
            }
            "--artifact-dir" => {
                config.artifact_dir = parse_next(&mut args, "--artifact-dir")?;
            }
            "--no-minimize" => {
                config.minimize_on_failure = false;
            }
            "--minimize-secs" => {
                let secs: u64 = parse_next(&mut args, "--minimize-secs")?;
                config.minimize_time = Duration::from_secs(secs);
            }
            "--target" => {
                let target = args
                    .next()
                    .ok_or_else(|| "--target needs a value".to_string())?;
                config.fuzz_targets = vec![parse_target(&target)?];
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(config)
}

fn parse_next<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<T, String> {
    let value = args.next().ok_or_else(|| format!("{name} needs a value"))?;
    value
        .parse()
        .map_err(|_| format!("invalid value for {name}: {value}"))
}

fn parse_target(value: &str) -> Result<FuzzTarget, String> {
    match value {
        "all" => Ok(FuzzTarget::All),
        "map" => Ok(FuzzTarget::Map),
        "list" => Ok(FuzzTarget::List),
        "text" => Ok(FuzzTarget::Text),
        "tree" => Ok(FuzzTarget::Tree),
        "movable-list" | "movable_list" => Ok(FuzzTarget::MovableList),
        "counter" => Ok(FuzzTarget::Counter),
        _ => Err(format!("unknown target: {value}")),
    }
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p fuzz --release --bin long_peer_fuzz -- [options]

Options:
  --seed <u64>                 Seed for deterministic replay (default: 1)
  --peers <u8>                 Number of peers (default: 8)
  --ops <u64>                  Operation cap, 0 means no cap (default: 10000)
  --duration-secs <u64>        Time cap; if --ops is omitted, run until this cap
  --sync-barrier-every <u64>   Force SyncAll every N ops, 0 disables (default: 2000)
  --check-every <u64>          Slow local checks every N ops, 0 disables (default: 5000)
  --recent-actions <usize>     Actions printed on failure (default: 64)
  --nested-containers          Also insert child containers into maps/lists/tree meta
  --artifact-dir <path>        Repro output directory (default: long_peer_fuzz_artifacts)
  --no-minimize                Write the full repro only
  --minimize-secs <u64>        Time budget for shrinking on failure (default: 30)
  --target <name>              all|map|list|text|tree|movable-list|counter"
    );
}
