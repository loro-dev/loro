# Grouped undo and handler follow-up

Measured on the same Tonbo ARM64 machine with Rust 1.100.0 on 2026-11-18.
The initial PR snapshot is `4ff5274f`; the pre-stack baseline is `b005024c`.
All versions use the same native drivers and a fixed source peer ID.

## The first undo still composed a growing delta

The initial PR compacted insertions within one transaction, but a grouped item
still retained one compact record per commit and materialized/composed each
record during `LocalInverse::seal`.

The first follow-up used eight retained interleaved measurements per variant,
after one warmup. The table reports medians for separate commits grouped into
one undo item:

| Insertions / commits | Initial PR first undo | Coalesced first undo |
| --- | ---: | ---: |
| 1,000 | 7.880 ms | 0.094 ms |
| 5,000 | 171.936 ms | 0.316 ms |
| 20,000 | 2687.464 ms | 1.165 ms |

The original growth is approximately quadratic across these sizes. Coalescing
continuous insertion ranges across commits before materialization removes that
growth in this workload. The 20,000-edit-plus-first-undo total falls from
2765.274 ms to 79.111 ms. Pre-stack remains faster at 65.102 ms.

This intermediate coalesced variant also included the subsequently rejected
arena experiment below. It is not the final PR source. The final comparison
separately removes that experiment and measures the additional handler change.
Its raw data is retained in `followup-exploratory-measurements.jsonl` and
`followup-exploratory-summary.json`.

## Final comparison

The final run interleaved all 24 permutations of four variants after one warmup
per scenario and variant, pinned to the same CPU. No builds or tests ran
concurrently. There are 24 retained samples per cell and 800 successful
invocations including warmups. Times are medians in milliseconds:

| Scenario | Pre-stack | Initial PR | Final candidate | Final / pre-stack |
| --- | ---: | ---: | ---: | ---: |
| Text, 20k separate commits, no undo | 33.354 | 35.123 | 34.525 | +3.51% |
| Map, 2k small commits after 10k-container setup | 3.249 | 3.469 | 3.447 | +6.10% |
| Text, 20k separate commits, undo enabled | 64.653 | 73.914 | 74.668 | +15.49% |
| JSONPath, 1k scans over 2k entries | 406.143 | 420.085 | 387.622 | -4.56% |
| Root handler lookup, 100k | 13.281 | 16.292 | 16.450 | +23.86% |
| Child handler lookup, 100k | 15.683 | 18.510 | 17.250 | +9.99% |
| Text, 20k inserts in one transaction + first undo | 8.879 | 9.969 | 10.064 | +13.34% |
| Text, 20k grouped commits + first undo | 65.163 | 2760.624 | 80.463 | +23.48% |

Only the last two rows include first-undo time. Other rows time editing/commits
or queries; snapshot round trips and undo/redo assertions run outside the timer.
Memory budgets are not configured. The text and map rows without undo do not
create an UndoManager. These native microbenchmarks do not establish WASM or
application-level performance, and they do not measure retained memory.

In the final grouped case, first undo alone is 2682.228 ms in the initial PR,
1.176 ms in the final candidate, and 0.228 ms pre-stack. The edit-plus-undo total
still exceeds pre-stack by 23.48%; eliminating the quadratic cost does not
eliminate recording overhead.

`BasicHandler::with_doc_state` now locks the document's state directly without
temporarily cloning its `Arc`. The matched variant without this change
(`narrow`) takes 415.416 ms for JSONPath, 18.570 ms for child lookup, and 16.896 ms
for root lookup. With it (`borrowed`), those times become 387.622, 17.250, and
16.450 ms. The final candidate still has a small editing cost versus the initial
PR: separate undo commits are 1.02% slower and grouped editing before undo is
1.71% slower. The raw samples retain this tradeoff.

## Reproducing the final measurements

`followup-summary.json` contains retained samples and medians;
`followup-measurements.jsonl` also retains stdout, exit status and warmups.
`followup-binary-sha256.json` records the measured binaries. Variant names are:

- `prestack`: `b005024cbd896e394cc875699a0c7f6cfa272a9e`.
- `pr`: initial PR commit `4ff5274f998a41356beee99f8f22eb8f2f3cbea3`.
- `narrow`: final production source with `BasicHandler::with_doc_state` restored
  to cloning `self.doc.state` before locking it.
- `borrowed`: final production source in this PR.

For each variant, copy the same `perf_hot_paths.rs` and `perf_followup.rs`
drivers into `crates/loro/examples/`, and build with Rust 1.100.0 on Linux ARM64:

```sh
cargo build --locked -p loro --release --features jsonpath \
  --example perf_hot_paths --example perf_followup
```

Use separate build directories. Put the eight binaries in a fresh directory's
`binaries/` subdirectory, named `<variant>-hot` and `<variant>-followup`, then run:

```sh
python3 benchmarks/hot-paths-20261118/measure-followup.py /path/to/run
```

This script preserves the final run's ordering and timing protocol; its output
path is configurable and it refuses to overwrite an existing raw data file.
Individual first-undo scaling probes use `perf_followup grouped 1000`,
`perf_followup grouped 5000`, and `perf_followup grouped 20000`.

## A read-lock fast path was rejected

Looking up an existing arena index before acquiring the registration write lock
avoided a redundant dirty mark, but slowed 100,000 root-handler lookups from
16.143 ms to 16.797 ms. Child lookup changed only from 18.477 ms to 18.348 ms.
The experiment was removed instead of making it part of the final patch.

## The new rich-text tests exposed an existing inverse mismatch

With `LORO_VERIFY_UNDO_LOG=1`, seed 3 of the styled Unicode mixed-edit test fails
on both unmodified `2370ab89` and the optimization prototype. Both source
versions were explicitly rebuilt after extraction; the failure occurs at their
respective comparator locations, not in a reused binary.

The checkout and recorded inverse place a restored styled `e` at different
positions within deleted input: delete 56 then insert, versus delete 34, insert,
delete 22. The test's visible text/style assertions pass without the oracle,
which is insufficient to establish equivalent transforms against later remote
insertions. Mixed inverses containing both restored and deleted text in the
same container now fall back to checkout, including after composing a group.
This is a deliberate correctness tradeoff: those mixed replacements may build
history caches. Pure insertion and pure deletion inverses remain recorded.

A separate comparator issue expanded a replacement's deletion before its
insertion, contrary to the delta iterator's execution order. The corrected
checker has positive tests including transforms against remote insertion at
every nearby position, and a negative test that rejects moving an insertion
across deleted input. This correction alone did not eliminate the mixed-edit
mismatch, so the fallback is independently required.

New public tests cover 96 styled Unicode seeds with batched/grouped insertions,
mixed deletion, style changes, undo and redo; 64 concurrent remote-edit seeds;
five remote insertion positions against a coalesced group; and undo-manager
replacement.

## Validation and remaining work

The final production source and drivers were compared byte-for-byte with the
source archived from Tonbo. With `LORO_VERIFY_UNDO_LOG=1` for debug tests:

- Focused hot-path and compact-text tests: 7 passed.
- `cargo test --locked -p loro-internal --lib --all-features`: 644 passed,
  4 ignored.
- `cargo test --locked -p loro --tests --features loro-internal/test_utils,jsonpath`:
  1,055 passed, 29 ignored, including the focused tests.
- Release Loom `multi_thread_test` and `global_registry_loom`, with
  `LOOM_MAX_PREEMPTIONS=2`: 11 passed.
- Local formatting and whitespace checks passed.

These results describe `f8654ac1`. Its initial draft-gated GitHub jobs were
skipped; after the PR became ready, the build and loro.js jobs passed on
2026-11-18. Subsequent review found broader grouped-undo and scalar-traversal
regressions; their fixes and measurements are in [review-fixes.md](review-fixes.md).
WASM, browser runtime, broad fuzzing, and memory measurements were not repeated
in this native follow-up.

The remaining measured targets are scalar root lookup (+23.86%), undo-enabled
editing (+15.49%), and grouped editing plus undo (+23.48%). Root lookup still
constructs and drops an owned document handle, enters the existing final-handle
gate, and registers its container. Separating these costs is a next experiment;
the rejected read-lock change does not establish which dominates. For undo,
the next candidates are per-commit record allocation and lifetime bookkeeping.
They need allocation/profile evidence and the same atomicity, memory-budget and
concurrent-drop checks before a further representation change.
