# Native commit and query measurements

Tonbo `loro`, Linux aarch64, Rust 1.100.0, measured 2026-11-18.

These are the initial PR measurements. Subsequent investigation and validation
are recorded in [the follow-up](followup.md), [review fixes](review-fixes.md),
and [round 2 hot-path and complexity optimizations](round2-optimizations.md).

- `prestack`: `b005024cbd896e394cc875699a0c7f6cfa272a9e`.
- `candidate`: `2370ab89f8306bfd5cc13c884641d18fef21a5bc` plus the earlier
  transaction admission, undo-registration and decoded-read fast paths.
- `combined`: the candidate plus compact text inverses, transaction-mark
  capacity recovery and shared ownership during bulk handler traversal.

Build the same `crates/loro/examples/perf_hot_paths.rs` driver for each version:

```sh
cargo build --locked -p loro --release --features jsonpath --example perf_hot_paths
```

The driver does not configure memory budgets. Only the undo scenarios create
an UndoManager. Timings cover edits/commits or queries, excluding document
setup and subsequent undo/redo and snapshot correctness checks.

Pin the processes to the same CPU, discard one warmup for each scenario and
binary, and run each of the six three-binary orderings twice in shuffled order.
Do not run builds or other tests concurrently. All 390 invocations succeeded.
`summary.json` contains 12 retained samples per cell; `measurements.jsonl`
contains raw output, exit codes and warmups.

| Scenario | Pre-stack ms | Candidate ms | Combined ms | Combined / pre-stack |
| --- | ---: | ---: | ---: | ---: |
| Text, 20k separate commits, no undo | 33.324 | 35.313 | 35.095 | +5.32% |
| Map, 2k small commits after 10k-container setup | 3.248 | 3.876 | 3.465 | +6.67% |
| Text, 20k separate commits, undo enabled | 64.164 | 83.704 | 73.726 | +14.90% |
| JSONPath, 1k scans over 2k entries | 405.933 | 475.885 | 419.352 | +3.31% |
| Pending, ten reversed-update imports | 452.106 | 487.370 | 460.899 | +1.94% |
| Root handler lookup, 100k | 13.306 | 16.392 | 16.336 | +22.77% |
| Child handler lookup, 100k | 15.666 | 17.770 | 18.463 | +17.85% |
| Text, 20k inserts in one transaction, undo enabled | 8.546 | 2723.808 | 9.850 | +15.26% |
| Text, 20k grouped commits, undo enabled | 64.450 | 108.258 | 77.775 | +20.67% |
| Map, ten batches of 10k edits | 63.740 | 66.295 | 65.576 | +2.88% |

These are medians of native microbenchmarks, not claims about application or
WASM performance. Ordinary child lookup regresses another 3.90% versus the
candidate; shared handler representation remains under investigation.
Pending varies across separately rebuilt drivers, so its improvement is not
independently attributed. Neither memory use nor first-undo latency is measured.

The combined production source passed the internal all-features library tests
(642 passed, 4 ignored), public tests with `test_utils,jsonpath` (1,051 passed,
29 ignored), and two Loom suites (11 passed, preemption bound 2). Debug tests
enabled `LORO_VERIFY_UNDO_LOG=1`. At this stage targeted rich-text, concurrent
undo and first-undo timing remained to be checked; see the follow-up for those
results.
