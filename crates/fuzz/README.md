# Fuzz Drivers

This crate contains both short regression fuzz tests and longer deterministic
drivers. The `long_peer_fuzz` driver is not a `cargo fuzz` target. It runs one
thread with many `LoroDoc` peers, applies random local edits, exchanges updates
between peers, and checks that all peers converge after synchronization.

## Long Peer Fuzz

Quick smoke run:

```bash
cargo run -p fuzz --bin long_peer_fuzz -- \
  --seed 1 \
  --peers 6 \
  --ops 2000 \
  --sync-barrier-every 400 \
  --check-every 1000
```

Long run:

```bash
pnpm long-peer-fuzz -- \
  --seed 20260629 \
  --peers 12 \
  --duration-secs 36000 \
  --sync-barrier-every 5000 \
  --check-every 20000 \
  --minimize-secs 120
```

Useful options:

- `--seed <u64>` fixes the generated action stream. Use the same seed and
  options to replay the same run.
- `--peers <u8>` controls the number of simulated peers.
- `--duration-secs <u64>` runs by wall clock time. If `--ops` is omitted, this
  is the only stop condition.
- `--ops <u64>` caps the number of generated actions. `--ops 0` means no op cap.
- `--target <name>` narrows the surface to `all`, `map`, `list`, `text`, `tree`,
  `movable-list`, or `counter`.
- `--sync-barrier-every <u64>` forces a `SyncAll` after every N generated ops.
- `--check-every <u64>` runs tracker and slow state checks after every N ops.
- `--artifact-dir <path>` chooses where crash repro files are written.
- `--minimize-secs <u64>` controls the best-effort shrinking budget after a
  crash.
- `--no-minimize` writes only the full repro.
- `--nested-containers` also inserts child containers into maps, lists, movable
  lists, and tree meta. This is more aggressive, but it can currently hit
  nested-container harness issues before the normal peer-convergence checks.

The driver also creates an active crash journal before it applies each raw
action. This matters for aborts caused by a second panic during unwinding; in
that case Rust may terminate the process before the normal failure handler can
run. The active journal directory contains:

- `actions.rs.inc`: raw actions appended before execution.
- `latest.txt`: the last action written, including op and phase.
- `repro_header.rs` and `repro_footer.rs`: wrappers for building a replay test.

To rebuild a replay from an active journal:

```bash
cat repro_header.rs actions.rs.inc repro_footer.rs > journal_repro.rs
cp journal_repro.rs crates/fuzz/tests/journal_repro.rs
cargo test -p fuzz --test journal_repro --release -- --nocapture
```

On ordinary unwindable failure, the driver writes a case directory under
`long_peer_fuzz_artifacts/` by default. It contains:

- `full_actions.txt`: all raw generated actions before preprocessing.
- `minimized_actions.txt`: the smallest action list found within the shrink
  budget.
- `full_repro.rs`: a Rust integration test for the full action list.
- `minimal_repro.rs`: a Rust integration test for the minimized action list.
- `README.md`: seed, peer count, failed phase, and replay instructions.

To replay a generated minimal repro, copy or move `minimal_repro.rs` under
`crates/fuzz/tests/`, then run:

```bash
cargo test -p fuzz --test minimal_repro
```

The generated repro stores raw `GenericAction` values. During replay, the fuzz
harness preprocesses them against the current document state, so the case stays
close to the original fuzz input rather than a post-processed trace.
