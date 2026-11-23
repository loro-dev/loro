# PR #1314 review fixes

Measured on 2026-11-18 on Tonbo `loro`, Linux ARM64, Rust 1.100.0.

## Changes

The general delta composer queried the deletion's exclusive end with a
right-biased output-length search. This skipped zero-output deletion fragments
at that position, so the update range included an already-deleted suffix.
Repeated backward deletions retained and revisited progressively more fragments.
This affected grouped text edits followed by a map write, groups editing two
texts, list pushes, and recording the inverse for list redo.

`DeltaRope::_compose_replace` now queries the last output unit being deleted and
advances that cursor by one. Existing deletions beyond the range remain intact;
the existing single-leaf update can merge adjacent fragments. In the 20,000-step
delta-only experiment the result retains two fragments instead of 20,000.

After fixing that scan, grouped sealing still cloned a temporary diff tree,
container map and order vector per commit. It now borrows stored diffs and
applies compact text inverse deletions directly after materializing the first
entry. Reverse commit order, inverse-map precedence, counter recomputation,
pruning, normalization and mixed-text checkout fallbacks remain unchanged.

List, movable-list and JSONPath map iteration now create a shared document owner
only upon encountering the first container value. Empty and scalar-only
traversals avoid the allocation and extra document drop-gate acquisition.
Callbacks still run after the attached state lock is released.

## Protocol

- `prestack`: `b005024c`, before the memory/undo stack.
- `main`: `2370ab89`, the merged stack, used for scalar comparisons.
- `head`: `f8654ac1`, the PR before these review fixes.
- `fixed`: the candidate identified by `review-final-source-sha256.txt`.

Release binaries use the default features and the same two drivers. The three
baseline binaries were retained from the review; all variants are measured anew
in the same machine activation. Each subprocess is pinned to the same CPU, with
no concurrent compilation or tests.

Grouped workloads use all six orderings of three variants after one discarded
warmup, giving six retained samples per cell. Scalar workloads use four rotations
and their reversals after warmup, giving eight samples per cell. Delta-only
measurements alternate the two versions for six retained samples. Each grouped
invocation asserts the document state after undo and redo; scalar invocations
assert value types and visit counts.

`review-grouped.rs`, `review-scalars.rs` and `measure-review-fixes.py` are the exact
drivers used remotely. To reproduce, copy the Rust files into each source tree's
`crates/loro/examples/` as `perf_pr1314_review.rs` and `perf_pr1314_scalars.rs`, then
build with `cargo build --locked -p loro --release --example perf_pr1314_review
--example perf_pr1314_scalars`. Put baseline executables in
`<parent>/pr1314-review/binaries/{prestack,main,head}` and corresponding
`*-scalars` paths; put candidate executables in `<run>/binaries/fixed` and
`fixed-scalars`. Run `python3 measure-review-fixes.py <run>` using an empty output
directory. The committed `review-final-*-measurements.jsonl` files retain every
invocation, including warmups and exit codes.

## Final measurements

All 708 final invocations succeeded: 378 grouped, 288 scalar and 42 delta-only.
Times below are medians in milliseconds.

First undo after 20,000 grouped commits:

| Workload | Pre-stack | PR before review fixes | Fixed |
| --- | ---: | ---: | ---: |
| Continuous text inserts | 0.188 | 1.122 | 1.138 |
| Continuous text inserts, then one map write | 0.254 | 2677.095 | 5.340 |
| One insertion in each of two texts per commit | 202.041 | 5526.255 | 159.541 |
| List push per commit | 17.407 | 2514.531 | 77.473 |
| Backward text deletion per commit | 52.807 | 24.033 | 20.234 |
| Separated text insertions | 230.607 | 158.733 | 151.326 |

The list redo changes from 2428.463 ms before the fixes to 40.068 ms; pre-stack
was 23.962 ms. These numbers include the work of recording the opposite-stack
inverse as well as applying the operation.

For the full edit + undo + redo cycle, medians are computed from each invocation's
sum, not by adding the component medians:

| Workload | Pre-stack | Fixed | Fixed / pre-stack |
| --- | ---: | ---: | ---: |
| Continuous text | 65.858 | 80.040 | 1.22 |
| Text plus final map write | 66.026 | 84.620 | 1.28 |
| Two texts | 501.249 | 344.742 | 0.69 |
| List pushes | 117.362 | 245.969 | 2.10 |
| Backward text deletion | 537.147 | 559.885 | 1.04 |
| Separated text insertion | 460.718 | 320.232 | 0.70 |

The scan is no longer quadratic in these probes. For example:

| Number of operations | Map-tail undo, before | Map-tail undo, fixed | Delta-only composition, fixed |
| --- | ---: | ---: | ---: |
| 1,000 | 7.791 | 0.306 | 0.428 |
| 5,000 | 171.180 | 1.359 | 2.119 |
| 20,000 | 2677.095 | 5.340 | 8.448 |

The first candidate, with the endpoint and lazy-owner fixes but without borrowed
sealing, took 13.705 ms for 20,000 map-tail undo. Borrowed sealing reduces that
to 5.340 ms. The first candidate's separate 708-invocation run is retained in
`/private/tmp/loro-pr1314-fix-evidence.tgz`; final raw samples are committed here.

100,000 scalar-only `for_each` calls:

| Container / length | Pre-stack | Merged stack | PR before review fixes | Fixed |
| --- | ---: | ---: | ---: | ---: |
| List / 0 | 5.699 | 6.098 | 12.418 | 5.388 |
| List / 1 | 10.163 | 10.322 | 16.740 | 9.919 |
| List / 8 | 34.863 | 36.038 | 46.629 | 36.494 |
| List / 128 | 327.660 | 327.020 | 346.624 | 334.723 |
| MovableList / 0 | 5.784 | 6.426 | 12.657 | 5.237 |
| MovableList / 1 | 10.439 | 10.823 | 16.955 | 10.376 |
| MovableList / 8 | 33.115 | 33.349 | 41.504 | 34.321 |
| MovableList / 128 | 559.298 | 568.430 | 581.063 | 570.227 |

## Validation

With `LORO_VERIFY_UNDO_LOG=1` for the Rust document tests:

- Final internal all-features library tests: 645 passed, 4 ignored.
- Final public tests with `loro-internal/test_utils,jsonpath`: 1,056 passed,
  29 ignored.
- Focused native undo/ownership/memory tests: 13 passed.
- Focused tests with `loro-internal/wasm` event coordinates: 8 passed.
- Delta tests: 47 passed, both in the workspace and against the published
  `generic-btree` 0.10.8 dependency outside the workspace.
- Formatting and whitespace checks passed.

After the full suites, the owner-count test was tightened to exclude temporary
global-registry references under the existing drop gate. The focused test passed
again. Rebuilding both release drivers after this test-only change produced
binaries byte-for-byte identical to those measured. All five changed Rust source
files and both drivers match the final remote source checksum manifest.

The new complexity regression counts length reads across 4,096 compositions
rather than asserting a wall-clock threshold. Restoring only the old endpoint
lookup makes it fail with 8,415,229 reads; the fixed endpoint passes. The delta
suite also checks boundary replacements and 8,192 seeded string replacements.
The public grouped regression covers a final map write, two texts, and lists,
five concurrent remote positions, and two undo/redo cycles.

The first validation attempt had a type error in the new movable-list test;
the next script used `test_utils` without its `loro-internal/` prefix. Both
validation mistakes were corrected, and the successful final commands cover
the complete candidate. Their original logs remain in the local evidence
archive. The expected negative-control failure is separate from those runs.

The native `wasm` feature run checks UTF-16 event-coordinate behavior; it is not
a WASM runtime benchmark. Browser/WASM runtime matrices, broad fuzzing, memory
measurements and Loom were not rerun for these review fixes.

## Limits

The endpoint fix removes the identified quadratic suffix scan. These experiments
do not establish a complexity bound for every delta shape or full recovery to
pre-stack performance. The inverse-recording and per-commit lifetime costs still
exist, and mixed text replacement groups still use checkout for correctness.

There are still paths above 2x pre-stack: first undo for the 20,000-list group is
4.45x and its full edit/undo/redo cycle is 2.10x. Map-tail first undo is 21.03x,
although its absolute cost is now 5.34 ms and its complete cycle is 1.28x.
Continuous grouped text first undo remains 6.07x (1.14 ms versus 0.19 ms).
These remaining costs must not be described as fully recovered or as proven
constant-time; the fixes remove the measured quadratic scan and scalar-owner
allocation regression.
