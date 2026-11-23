# CRDT benchmark hot paths

Both rounds use local release WASM builds on Apple M5 Pro with macOS 27.0.1 and
Node v24.22.1. The first-round temporary checkout was lost. Its recovered
source builds a Node WASM binary with the same SHA-256 as the original
`d05510b6` artifact. The restored commit has a different Git ID.

| Build | Source revision | Node WASM SHA-256 |
| --- | --- | --- |
| base | `d0afd2b06a1b827bac11b06bde6f717b3bd9b151` | `86ee2b727a784f0d36a4aa785e4118feff3aa588898f886a95bbeed482454442` |
| previous | `739c6d044c798d484aeba4f4282e4ba7365c5ae2` | `37c8a7b9e187ab4902f133ff64620e0de1646e34e9b0f9f5991c733122d70a7c` |
| candidate | `1da9b49821f9a073714bbd099948e75fd7548fd7` | `05424661140587e1d414b20983edab8e13bfa3731de3dcc9e07a49a4c66d10bb` |

The [benchmark checkout](https://github.com/zxch3n/crdt-benchmarks/tree/e699425e5fd71300183c4cc4b21b210534211733)
uses lib0 0.2.117 and Yjs 13.6.32, as in the first round. Loro results are medians
of three fresh processes per build, with the starting build rotated. Each suite
runs B1, B2, B3, B4 and C1 in order. The isolated B4 timings use separate
processes. The comparison ran on a shared machine while some validation jobs
were active. The Yjs column is one process per scenario. The benchmark itself
rounds readings to whole milliseconds. The standalone snapshot row times
`doc.import()` alone, with the same snapshot bytes across builds.
[Raw samples and build provenance](results.json)

| Scenario | Baseline ms | First round ms | Second round ms | Yjs ms |
| --- | ---: | ---: | ---: | ---: |
| B1.1 | 106 | 85 | 78 | 85 |
| B1.2 | 0 | 1 | 1 | 0 |
| B1.3 | 81 | 64 | 57 | 78 |
| B1.4 | 83 | 63 | 55 | 85 |
| B1.5 | 80 | 62 | 56 | 91 |
| B1.6 | 1 | 1 | 1 | 1 |
| B1.7 | 96 | 80 | 76 | 92 |
| B1.8 | 94 | 80 | 76 | 145 |
| B1.9 | 7 | 7 | 7 | 2 |
| B1.10 | 87 | 68 | 64 | 75 |
| B1.11 | 85 | 70 | 65 | 76 |
| B2.1 | 1 | 1 | 1 | 0 |
| B2.2 | 27 | 26 | 32 | 27 |
| B2.3 | 35 | 34 | 38 | 38 |
| B2.4 | 73 | 68 | 75 | 57 |
| B3.1 | 18 | 15 | 12 | 30 |
| B3.2 | 24 | 19 | 19 | 31 |
| B3.3 | 30 | 29 | 38 | 71 |
| B3.4 | 91 | 79 | 45 | 26 |
| B3.5 | 85 | 80 | 44 | 25 |
| B4 in suite | 4,312 | 964 | 673 | 1,500 |
| C1.1 | 1,415 | 1,383 | 1,528 | 25,230 |
| C1.1 parseTime | 36 | 35 | 39 | 1,630 |
| C1.2 | 1,292 | 1,156 | 1,325 | 19,440 |
| C1.2 parseTime | 400 | 327 | 389 | 7,163 |
| B4 isolated | 1,311 | 907 | 682 | 1,515 |
| 30,000-peer snapshot import only | 195.2 | 175.7 | 179.1 | — |

B3.4 and B3.5 still trail Yjs. The search for the right origin is logarithmic
in the B-tree size. Ordering future spans can still scan a linear prefix.
B3.3 and C1.1 took longer than the first round in median timings.
C1.1 took longer in all three suite samples. C1.2's
suite timing varied substantially, including a 5,657 ms candidate sample.
The standalone snapshot row provides an import-only comparison without the
suite's explicit GC and heap sampling.

## B4x100

All runs use `--expose-gc --max-old-space-size=2048`. The baseline and first-round
long-run columns use the retained first-round measurements. The second round
runs the same 25,977,800 edits, then exports and reloads a snapshot. Each loop
includes 182,315 one-unit UTF-16 inserts and 77,463 deletes. RSS is a
process high-water mark. V8 heap figures are sampled every 8,192 edits, with
startup and final samples. The failed baseline figure is from its final GC log.

| Metric | Baseline | First round | Second round | Yjs |
| --- | ---: | ---: | ---: | ---: |
| Completion | V8 OOM after 15% marker | 100% | 100% | 100% |
| Edit time | — | 92.431 s | 83.098 s | 165.081 s |
| Total wall time | 201.572 s to OOM | 92.897 s | 83.547 s | 166.777 s |
| Peak RSS | 2,356.4 MiB | 513.8 MiB | 520.1 MiB | 1196.1 MiB |
| V8 heap used | 1,646.1 MiB at final GC | 65.5 MiB | 65.4 MiB | 681.2 MiB |

## Behavior checks

- A Node probe imported 1,549 independent text updates. The first-round
  build made 1,549 entropy calls during the import. The second round made
  zero, and both applied all 1,549 peers.
- Debug assertions compare direct singleton encoding against the general
  encoder and pre-merge bytes against the stored suffix. Tests cover
  backwards deletes, additional peers, changed pre-commit metadata,
  container operations, and large length prefixes.
- Concurrent list and text imports with shuffled input match cold replay
  and checkout. Debug builds check the B-tree search against a linear scan.
- The second-round WASM build produces byte-identical subscription updates
  for a mixed-container fixture. It exchanges updates and snapshots in both
  directions with each earlier build.

## Validation

- Format and Clippy passed. The Rust matrix passed 1,799 tests with
  `LORO_VERIFY_UNDO_LOG=1`. The internal doctest command passed four tests.
  The `jsonpath,loro-internal/wasm` feature matrix passed 1,120 tests.
- Both Loom suites passed, including concurrent subscription and batch import.
- Release WASM and separate package tests passed. Vitest passed 589 tests.
  Declaration, CommonJS, Deno, Bun and all 14 bundler smoke checks passed.

## Third round: cold snapshot memory, O(1) head summary, and merge peak memory

Matched 3-sample medians and WASM allocator measurements on the same Apple M5 Pro
(`Node v24.22.1`) comparing the second-round build (`1da9b498`) against the
third-round build:

| Metric | Second round (`1da9b498`) | Third round | Improvement |
| --- | ---: | ---: | ---: |
| B3.3 `parseTime` (in suite / isolated) | 98 ms / 91 ms | **51 ms / 33 ms** | **1.92x–2.76x faster** |
| B3.3 loaded doc `changeCacheBytes` | 23.20 MiB | **0 B** | **−100%** |
| B3.3 loaded doc `totalBytes` | 27.84 MiB | **4.37 MiB** | **6.36x smaller** |
| C1.2 `parseTime` (in suite / isolated) | 417 ms / 419 ms | **115 ms / 85 ms** | **3.63x–4.93x faster** |
| 30,000-peer snapshot `doc.import()` only | 164.0 ms | **44.3 ms** | **3.70x faster** |
| C1.2 loaded doc `changeCacheBytes` | 17.64 MiB | **0 B** | **−100%** |
| C1.2 loaded doc `dagBytes` | 6.64 MiB | **1.37 MiB** | **4.8x smaller** |
| C1.2 loaded doc `totalBytes` | 28.60 MiB | **5.70 MiB** | **5.02x smaller** |
| C1.2 `time` (in suite) | 744 ms | **704 ms** | **−5.4%** |
| C1.1 `peakAllocatedHeapBytes` | 324.97 MiB | **258.08 MiB** | **−66.89 MiB (−20.6%)** |
| C1.1 `wasmLinearMemoryBytes` | 358.94 MiB | **305.19 MiB** | **−53.75 MiB (−15.0%)** |
| C1.1 `time` (in suite) | 1,649 ms | **1,616 ms** | **−2.0%** |
| B4 `time` (in suite / isolated) | 841 ms / 723 ms | **795 ms / 711 ms** | **−5.5% / −1.7%** |

Subsequent local writes on the loaded `B3.3` and `C1.2` documents execute in
`1.60 ms` and `9.18 ms` while keeping cold peer blocks unparsed
(`changeCacheBytes` stays `6.4 KiB` and `117.6 KiB` rather than `23.20 MiB` and
`17.75 MiB`).

## Reproduce

Build each checkout with `pnpm release-wasm`. Copy [run.mjs](run.mjs) into the
benchmark checkout, and point its `node_modules/loro-crdt` at the built package.

```sh
node --expose-gc --max-old-space-size=2048 run.mjs loro full /tmp/loro-results.json
```

Choose `b4` for an isolated process or `b4x100` for the long run. Use `yjs`
in place of `loro` to measure Yjs. [parse-snapshot.cjs](parse-snapshot.cjs)
takes `prepare` or `sample`, the absolute package Node entrypoint, and a
snapshot path. Use `--expose-gc` when sampling. [verify-wire.cjs](verify-wire.cjs)
takes two absolute Node entrypoints.
