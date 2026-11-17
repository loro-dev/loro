# Single-document budget commit probe

Verified against code 2026-11-16.

Same `crates/loro/examples/memory_budget_commit_bench.rs` source for both binaries:
10,000 materialized maps, mutate one field, 2,000 ordinary commits, 1 GiB hard
quota (no pressure), and a no-budget control. Each binary ran twice in reversed
order, 7 rounds per invocation; medians below contain all 14 rounds. Setup is
outside the timer; final contents and usage below the quota are asserted.

| Binary | No budget, ms | Doc quota, ms |
| --- | ---: | ---: |
| Original #1300 | 1.2994 | 76.3982 |
| Repaired #1300 plus lower-layer repairs | 1.3832 | 2.5303 |

The quota case is about 30.2 times faster in this narrow workload. The unbudgeted
control is about 6.5% slower in this run; this probe does not establish that every
unbudgeted workload is unchanged or isolate which lower-layer repair contributes.
This is not a global-budget, WASM, cold-import or full B4 measurement.

The baseline executable was linked in this run against the **existing native
rlib** from the earlier investigation's `target-p3`, tagged `35510eff` in its
`versions.json`. That rlib was not rebuilt here. The associated `InnerStore`
source was byte-compared to `git show 35510eff:.../inner_store.rs` and matched.
Treat this as a reused investigation baseline, not a newly rebuilt old revision.
`manifest.json` records source/binary SHA256; CSV files are raw results.

Current build command:

```sh
cargo build -p loro --release --example memory_budget_commit_bench
./target/release/examples/memory_budget_commit_bench
```

Debug regression tests independently check cached totals against a complete
recount and count page recomputations rather than asserting elapsed time.
