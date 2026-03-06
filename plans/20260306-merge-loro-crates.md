# Plan: Merge `crates/loro` and `crates/loro-internal` into One Crate

Date: 2026-03-06
Status: Done
Primary package target: `loro`
Compatibility stance: Compatibility-first by default, with explicit decision points for semver-breaking cleanup

## How to Use This Document

- Update each phase status as work progresses: `Not Started`, `In Progress`, `Blocked`, or `Done`.
- Each merged PR should update the relevant checklist items in this document.
- A phase is only `Done` when all exit criteria are satisfied.
- If a major design decision changes the plan, update the "Decision Log" section before continuing.

## Background

Today the Rust implementation is split across two crates:

- `crates/loro`: the public, documented, stable-facing facade.
- `crates/loro-internal`: the engine crate that contains most of the implementation.

This split currently creates two distinct problems:

1. It duplicates API layers and type surfaces.
2. It introduces avoidable conversion work between those layers.

The highest-cost conversions are not the outer `LoroDoc` wrapper by itself. The main overhead lives in:

- event bridging in `crates/loro/src/event.rs`
- `ValueOrHandler` to `ValueOrContainer` conversions
- `Diff` and `DiffBatch` re-materialization
- container handle re-wrapping around internal handlers

The split also creates long-term maintenance cost:

- public semantics live in one crate
- engine behavior lives in another crate
- first-party consumers such as `crates/loro-wasm` depend directly on low-level internal APIs

As a result, physically moving files without collapsing the duplicate type layers would reduce organizational complexity, but it would not remove the main runtime overhead we care about.

## Purpose

This plan aims to make `loro` the only published Rust crate for the core implementation while preserving correctness and keeping migration risk controlled.

The target state is:

- one canonical implementation crate: `loro`
- one canonical `LoroDoc`
- one canonical container/value/diff/event surface
- no first-party crate depending on `loro-internal`
- no repeated facade-to-engine conversion on hot paths unless explicitly kept as compatibility shims

## Goals

- Merge `crates/loro` and `crates/loro-internal` into one canonical Rust crate.
- Preserve current public semantics that users rely on, especially auto-commit defaults.
- Remove duplicated type layers where they materially affect hot paths.
- Keep the workspace buildable during the transition.
- Give each migration stage explicit entry criteria, deliverables, exit criteria, and validation steps.

## Non-Goals

- Rewriting the CRDT engine.
- Redesigning unrelated APIs during the merge.
- Removing every escape hatch in the first iteration.
- Changing JS and WASM behavior unless required by the Rust merge.
- Chasing cosmetic file movement without measurable simplification or performance payoff.

## Hard Constraints

- `loro::LoroDoc::new()` must preserve current auto-commit behavior unless an explicit public API decision says otherwise.
- Import/export, diff, checkout, undo, and subscription correctness must not regress.
- The `loro-wasm` pending-event flush invariant must remain valid.
- The workspace must remain buildable at every phase boundary.
- Performance-sensitive changes must be validated with targeted benchmarks, not only with compilation success.

## Success Metrics

- `crates/loro` no longer depends on `loro-internal`.
- No first-party crate imports `loro-internal`.
- The event path no longer requires the current facade-only `DiffEvent` reconstruction on first-party hot paths.
- Heterogeneous read paths no longer require repeated `ValueOrHandler` to `ValueOrContainer` conversion on canonical APIs.
- `crates/loro/tests` remains a valid public compatibility suite.
- Public crate documentation continues to live with `loro`, not with a hidden engine crate.

## Current-State Summary

- `crates/loro/src/lib.rs` wraps `InnerLoroDoc` and internal handlers with public facade types.
- `crates/loro/src/event.rs` reconstructs event, diff, and batch types from internal representations.
- `crates/loro-internal/src/lib.rs` exposes far more engine surface than should become a long-term public contract.
- `crates/loro-wasm` imports many low-level items from `loro-internal`, so the merge must include a consumer migration plan, not only a crate move.

## Tracking Dashboard

| Phase | Name | Status | Depends On | Main Output |
| --- | --- | --- | --- | --- |
| 0 | Lock behavior and perf baseline | Done | None | Baseline tests and benchmark numbers |
| 1 | Import engine into `loro` | Done | Phase 0 | `loro` owns implementation modules |
| 2 | Merge canonical `LoroDoc` semantics | Done | Phase 1 | One canonical document type |
| 3 | Collapse container and value surface | Done | Phase 2 | One canonical container/value layer |
| 4 | Collapse event, diff, and undo surface | Done | Phase 3 | One canonical event/diff/undo layer |
| 5 | Migrate `loro-wasm` and first-party consumers | Done | Phase 4 | No first-party dependency on `loro-internal` |
| 6 | Remove shim and finalize cleanup | Done | Phase 5 | Single-crate steady state |

## Phase 0: Lock Behavior and Performance Baseline

Status: Done

### Objective

Freeze the current public behavior and record baseline performance before changing crate boundaries.

### Why This Phase Exists

Without a baseline, later phases can accidentally change semantics while still compiling cleanly. This phase turns the current behavior into an explicit contract.

### Primary Scope

- Public Rust behavior exercised through `crates/loro`
- Engine correctness currently covered by `crates/loro-internal`
- Performance-sensitive paths affected by facade-to-engine conversions

### Work Items

- [x] Inventory the public semantics currently provided only by `crates/loro`.
- [x] Treat `crates/loro/tests` as the public compatibility suite.
- [x] Identify the minimum subset of `crates/loro-internal/tests` that must remain green throughout the migration.
- [x] Add or confirm benchmark coverage for:
  - active subscriptions
  - heterogeneous reads
  - diff/apply-diff paths
  - undo callbacks
- [x] Record baseline commands and store baseline numbers in the PR or a linked artifact.

### Deliverables

- A written baseline summary
- A stable list of compatibility tests
- Benchmark numbers for the key hot paths

### Exit Criteria

- Public semantics are enumerated and agreed upon.
- Required tests and benchmarks are identified and runnable.
- Baseline measurements are captured once on the current split architecture.

### Validation

- `cargo test -p loro`
- `cargo test -p loro-internal`
- `cargo bench -p loro-internal --features test_utils --bench event -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2`
- `cargo bench -p loro-internal --features test_utils --bench pending -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2`
- `cargo bench -p loro-internal --features test_utils --bench list -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2`
- `cargo bench -p loro --bench merge_baseline -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2`

### Baseline Summary

Captured on 2026-03-06 in the current split-crate architecture.

- Public compatibility suite: all of `crates/loro/tests/**`.
- Public constructor and attached-container auto-commit semantics are now explicitly locked by `crates/loro/tests/merge_semantics_baseline.rs`:
  - `LoroDoc::new()`
  - `LoroDoc::from_snapshot()`
  - `LoroDoc::fork_at()`
  - attached container `doc()`
- Minimum `loro-internal` tests to keep green during the merge:
  - `crates/loro-internal/tests/autocommit.rs`
  - `crates/loro-internal/tests/test.rs`
  - `crates/loro-internal/tests/undo.rs`
  - `crates/loro-internal/tests/richtext.rs`
  - `crates/loro-internal/tests/tree.rs`
- Existing engine benches remain the baseline for split-architecture internals:
  - `crates/loro-internal/benches/event.rs`
  - `crates/loro-internal/benches/pending.rs`
  - `crates/loro-internal/benches/list.rs`
- Added a public-facade perf baseline in `crates/loro/benches/merge_baseline.rs` for:
  - active subscriptions
  - heterogeneous reads
  - diff/apply-diff
  - undo callbacks
- Refreshed `crates/loro-internal/benches/pending.rs` and `crates/loro-internal/benches/list.rs` so the documented benchmark commands match the current text-handler APIs and are runnable again.

### Baseline Numbers

One-shot local Criterion samples with short warm-up/measurement windows; use them as coarse merge checkpoints, not publication-grade statistics.

| Scope | Benchmark | Time |
| --- | --- | --- |
| internal event | `resolved/subContainer in event` | `928.24 ms .. 995.86 ms` |
| internal pending | `B4 pending decode/detached mode` | `52.171 ms .. 53.067 ms` |
| internal list | `10 list containers/sync random inserts to 10 list containers` | `71.750 ms .. 74.392 ms` |
| internal list | `many_actors/100 actors` | `127.92 ms .. 131.60 ms` |
| public facade | `merge baseline/public active subscriptions` | `10.303 us .. 12.682 us` |
| public facade | `merge baseline/public heterogeneous reads` | `1.4776 us .. 1.5084 us` |
| public facade | `merge baseline/public diff apply_diff` | `11.349 us .. 12.090 us` |
| public facade | `merge baseline/public undo callbacks` | `9.3043 us .. 10.831 us` |

### Risks

- Missing a public semantic edge case and treating it as implementation detail later
- Measuring the wrong paths and optimizing the wrong layer

## Phase 1: Import the Engine into `loro`

Status: Done

### Objective

Make `loro` own the implementation modules while preserving the current public behavior.

### Strategy

The recommended strategy is to move the implementation into `crates/loro` first, then reduce duplicate surfaces in later phases. This keeps the published crate name stable while avoiding a large semantic rewrite in the same step.

### Primary Scope

- `crates/loro/Cargo.toml`
- `crates/loro/src/**`
- `crates/loro-internal/Cargo.toml`
- `crates/loro-internal/src/**`

### Work Items

- [x] Create an internal module tree inside `crates/loro` to host the current engine implementation.
- [x] Merge the dependency sets from `crates/loro` and `crates/loro-internal`.
- [x] Merge feature flags while preserving the public `loro` feature contract.
- [x] Make `crates/loro` compile against the local engine modules instead of the path dependency on `loro-internal`.
- [x] Convert `crates/loro-internal` into a temporary compatibility shim that re-exports from `loro`.
- [x] Keep the workspace buildable through the shim while downstream consumers are still migrating.

### Deliverables

- `loro` builds without a path dependency on `loro-internal`
- `loro-internal` still exists as a temporary forwarding crate
- No intentional public behavior changes yet

### Exit Criteria

- `cargo tree -p loro` no longer shows `loro-internal` as a dependency edge.
- Public tests still target `loro` and remain green.
- The compatibility shim is sufficient for first-party crates to continue building.

### Validation

- `cargo test -p loro`
- `cargo test -p loro-internal`
- workspace build check

### Phase 1 Summary

Completed on 2026-03-06.

- Imported the engine source tree into `crates/loro/src/internal/**` and retargeted facade imports from `loro_internal::...` to `crate::internal::...`.
- Merged the dependency and feature surface into `crates/loro/Cargo.toml`, including the internal-only support features needed by the compatibility shim.
- Converted `crates/loro-internal` into a forwarding crate that re-exports `::loro::internal::*` and the macro surface used by existing first-party tests, benches, and examples.
- Updated the public compatibility suite under `crates/loro/tests/**` so it targets `loro` directly rather than importing `loro_internal`.
- Validation passed:
  - `cargo tree -p loro` no longer shows `loro-internal` as a dependency edge.
  - `cargo test -p loro`
  - `cargo test -p loro-internal`
  - `cargo check --workspace`

### Risks

- Import cycles or accidental feature drift
- Moving too much public surface into the root of `loro`

## Phase 2: Merge Canonical `LoroDoc` Semantics

Status: Done

### Objective

Remove the outer `LoroDoc { doc: InnerLoroDoc }` split and make one canonical `LoroDoc` type own the merged behavior.

### Primary Scope

- document constructors
- auto-commit behavior
- `fork`, `fork_at`, `from_snapshot`
- container `doc()` behavior
- public escape hatches such as `inner()`, `with_oplog`, and `with_state`

### Work Items

- [x] Move the current public constructor semantics into the canonical merged `LoroDoc`.
- [x] Preserve current auto-commit behavior for:
  - `new()`
  - `from_snapshot()`
  - `fork_at()`
  - `doc()` returned from attached containers
- [x] Decide whether `inner()` survives, is deprecated, or is replaced by a narrower API.
- [x] Decide the long-term shape of `with_oplog` and `with_state`.
- [x] Ensure first-party constructors such as those in `loro-wasm` keep the same behavior.

### Deliverables

- One canonical `LoroDoc`
- No public behavior regression in document lifecycle APIs

### Exit Criteria

- The public `LoroDoc` is no longer a facade around a second document type.
- All known auto-commit semantics match the baseline.
- Escape-hatch behavior is explicitly documented, not accidental.

### Validation

- `cargo test -p loro`
- targeted tests for:
  - `new()`
  - `from_snapshot()`
  - `fork_at()`
  - attached container `doc()`

### Phase 2 Summary

Completed on 2026-03-06.

- Public `LoroDoc` now stores the canonical `Arc<LoroDocInner>` directly instead of wrapping an `InnerLoroDoc` value.
- The public constructor and lifecycle methods still preserve the Phase 0 auto-commit semantics for:
  - `new()`
  - `from_snapshot()`
  - `fork_at()`
  - attached container `doc()`
- `inner()` survives as a compatibility escape hatch returning an internal `LoroDoc` view backed by the same `Arc<LoroDocInner>`.
- `with_oplog()` and `with_state()` are retained unchanged as the current escape hatches for low-level access; narrowing or deprecation is deferred until the container/value/event collapse is farther along.
- Validation passed:
  - `cargo test -p loro`
  - `cargo test -p loro --test merge_semantics_baseline`

### Risks

- Silent drift between manual-commit and auto-commit behavior
- Accidentally widening low-level lock or state APIs into permanent public contract

## Phase 3: Collapse the Container and Value Surface

Status: Done

### Objective

Remove the duplicated container and value layers where the facade currently wraps internal handlers and value enums.

### Primary Scope

- `LoroList`, `LoroMap`, `LoroText`, `LoroTree`, `LoroMovableList`, `LoroCounter`
- `Container`
- `ValueOrContainer`
- `ContainerTrait`
- handler-to-container and value-to-value conversion paths

### Work Items

- [x] Choose the canonical naming strategy for container handles.
- [x] Decide whether to keep public names such as `LoroText` as aliases, renamed canonical types, or compatibility wrappers.
- [x] Collapse `Container` and the internal handler enum into one canonical representation where practical.
- [x] Collapse `ValueOrContainer` and `ValueOrHandler` into one canonical representation where practical.
- [x] Remove or narrow `ContainerTrait` if it only exists to bridge two type layers.
- [x] Eliminate conversion-heavy read paths from:
  - list/map getters
  - `for_each`
  - `values`
  - `get_by_path`
  - `get_by_str_path`
  - `jsonpath`

### Deliverables

- One canonical container handle layer
- One canonical value-or-container layer
- Reduced wrapper churn on read-heavy paths

### Exit Criteria

- Canonical APIs no longer need the current repeated handler-to-container and value-to-value wrapping.
- Public names are stable or explicitly compatibility-shimmed.
- Documentation for the canonical container types exists in `loro`.

### Validation

- `cargo test -p loro`
- read-path regression tests
- benchmark comparison against Phase 0 heterogeneous-read baseline

### Phase 3 Summary

Completed on 2026-03-06.

- Adopted a compatibility-first naming strategy:
  - `loro::internal::{Handler, ValueOrHandler, ListHandler, MapHandler, TextHandler, TreeHandler, MovableListHandler}` is the canonical first-party container/value surface.
  - Public `LoroList`, `LoroMap`, `LoroText`, `LoroTree`, `LoroMovableList`, `Container`, and `ValueOrContainer` remain compatibility wrappers for the stable public facade.
- Re-exported the canonical handler/value types from the `loro::internal` root so first-party consumers no longer need deep `handler::...` imports to stay on the merged engine surface.
- Added `crates/loro/tests/internal_canonical_surface.rs` to lock the canonical internal read-path behavior around:
  - list/map getters
  - nested container discovery via `ValueOrHandler`
  - `for_each`
  - `values`
  - `get_by_str_path`
- Extended `crates/loro/benches/merge_baseline.rs` with an internal heterogeneous-read benchmark that exercises the canonical handler/value surface directly.
- Validation passed:
  - `cargo test -p loro`
  - `cargo bench -p loro --bench merge_baseline -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2`
- Benchmark comparison against the Phase 0 public-facade baseline:
  - `merge baseline/public heterogeneous reads`: `1.5213 us .. 1.6324 us`
  - `merge baseline/internal heterogeneous reads`: `1.4115 us .. 1.4424 us`

### Risks

- Public type inference changes
- Loss of ergonomic names or public docs if aliasing is chosen poorly

## Phase 4: Collapse the Event, Diff, and Undo Surface

Status: Done

### Objective

Remove the current event and diff reconstruction layer, which is likely the highest-value runtime simplification in the merge.

### Why This Phase Is Separate

This is the most sensitive API surface. The internal and public event shapes are not identical today, so this phase needs an explicit decision rather than an implicit refactor.

### Primary Scope

- subscriptions
- `DiffEvent`
- `ContainerDiff`
- `Diff`
- `DiffBatch`
- undo callback payloads

### Decision Gate

Chosen on 2026-03-06: compatibility-first.

The canonical first-party event/diff/undo surface is `loro::internal::{DiffEvent, Diff, DiffBatch, UndoManager, UndoItemMeta, UndoOrRedo}`. The existing public `loro::event::*` and public `UndoManager` callback payloads remain compatibility wrappers.

Before implementation starts, choose one of these approaches and record it in the Decision Log:

- Compatibility-first: introduce a canonical borrowed or raw event API, keep the current owned event API as a compatibility layer for one or more releases.
- Break-now: replace the old event shape immediately in a semver-major change.

### Work Items

- [x] Select the canonical event model.
- [x] Select the canonical diff model.
- [x] Decide whether old `subscribe` remains temporarily as a compatibility wrapper.
- [x] Update `UndoManager` callback payloads to use the canonical event or diff types.
- [x] Remove first-party hot-path dependence on facade-only event reconstruction.
- [x] Re-run the active-subscription benchmark and compare against the Phase 0 baseline.

### Deliverables

- One canonical event and diff surface
- Measurable reduction in event-path allocations or conversion work

### Exit Criteria

- First-party hot paths no longer require the current `DiffEvent::from` bridge.
- Undo callbacks no longer need a duplicate event model.
- The chosen compatibility story is documented and enforced.

### Validation

- `cargo test -p loro`
- subscription-focused regression tests
- benchmark comparison against Phase 0 active-subscription baseline

### Phase 4 Summary

Completed on 2026-03-06.

- Chose the compatibility-first event model:
  - `loro::internal::{DiffEvent, Diff, DiffBatch, UndoManager, UndoItemMeta, UndoOrRedo}` is the canonical first-party event/diff/undo surface.
  - Public `loro::event::*`, public `subscribe*`, and public `UndoManager` remain compatibility wrappers over that canonical internal surface.
- Re-exported the canonical event, diff, subscription, and undo types from the `loro::internal` root so first-party crates can stay off the facade bridge without deep module imports.
- Extended `crates/loro/tests/internal_canonical_surface.rs` to lock:
  - internal `subscribe_root` delivering canonical `DiffEvent`
  - internal `UndoManager::set_on_push` delivering canonical `DiffEvent`
  - text diffs staying on the internal `Diff::Text` path end to end
- Extended `crates/loro/benches/merge_baseline.rs` with internal benchmarks for:
  - active subscriptions
  - undo callbacks
- Validation passed:
  - `cargo test -p loro`
  - `cargo bench -p loro --bench merge_baseline -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2`
- Benchmark comparison against the Phase 0 public-facade baseline:
  - `merge baseline/public active subscriptions`: `8.1305 us .. 8.5955 us`
  - `merge baseline/internal active subscriptions`: `7.8933 us .. 8.1270 us`
  - `merge baseline/public undo callbacks`: `9.0388 us .. 11.589 us`
  - `merge baseline/internal undo callbacks`: `9.0572 us .. 9.5479 us`

### Risks

- Public lifetime complexity if a borrowed event API becomes public
- Compatibility overhead if the old and new event APIs must coexist for too long

## Phase 5: Migrate `loro-wasm` and Other First-Party Consumers

Status: Done

### Objective

Remove first-party dependencies on `loro-internal` and make `loro` the only crate used by workspace consumers.

### Primary Scope

- `crates/loro-wasm`
- examples
- benches
- internal tools or support crates still depending on `loro-internal`

### Work Items

- [x] Switch `crates/loro-wasm` imports from `loro-internal` to `loro`.
- [x] If low-level support is still needed, expose a narrowly scoped internal support module from `loro` rather than exposing the entire engine root.
- [x] Keep the JS pending-event flush invariant intact.
- [x] Audit all workspace members and remove direct `loro-internal` imports.
- [x] Update examples and benches to use the merged crate surface.

### Deliverables

- No first-party crate depends directly on `loro-internal`
- `loro-wasm` builds against `loro`

### Exit Criteria

- A repository-wide search shows no first-party import of `loro-internal`, except the temporary shim crate itself.
- `loro-wasm` behavior remains correct.
- Any hidden internal support surface is intentionally scoped and documented as such.

### Validation

- `cargo test -p loro-wasm`
- `pnpm -C crates/loro-wasm build-release`
- repository-wide search for `loro_internal`

### Phase 5 Summary

Completed on 2026-03-06.

- Migrated `crates/loro-wasm` off the shim crate:
  - dependency switched from `loro-internal` to `loro`
  - low-level imports retargeted from `loro_internal::...` to `loro::internal::...`
- Kept the JS pending-event flush invariant intact; the release build and downstream JS/deno/bun test suites all passed after the migration.
- Confirmed the first-party repo no longer imports `loro-internal` outside the temporary shim crate:
  - the remaining `loro_internal::...` hits are coverage/fuzz string labels, not Rust imports
  - no `loro-internal = { ... }` dependency edges remain in first-party `Cargo.toml` files outside the shim
- Validation passed:
  - `cargo test -p loro-wasm`
  - `pnpm -C crates/loro-wasm build-release`
  - `rg -n "use loro_internal|loro_internal::|loro-internal\\s*=\\s*\\{" crates --glob '!crates/loro-internal/**' --glob '!target'`
- Non-blocking warnings observed during validation:
  - existing Rollup/TypeScript lib-target warnings in `crates/loro-wasm/index.ts`
  - existing npm user/env config warnings during `npm run test`

### Risks

- Accidentally promoting too much engine API to long-term public surface just to satisfy `loro-wasm`
- Breaking the event flush invariant while touching binding code

## Phase 6: Remove the Shim and Finalize Cleanup

Status: Done

### Objective

Delete the temporary compatibility crate and complete the transition to a true single-crate architecture.

### Primary Scope

- `crates/loro-internal`
- docs
- readmes
- release notes
- migration notes

### Work Items

- [x] Delete `crates/loro-internal`.
- [x] Remove any temporary compatibility aliases or forwarding code that no longer serves a migration purpose.
- [x] Consolidate remaining tests, benches, and docs into the merged `loro` crate.
- [x] Update crate documentation and repository documentation.
- [x] Write release notes and a migration guide if any user-visible API moved or changed.

### Deliverables

- No `loro-internal` crate in the workspace
- One source of truth for implementation and public documentation

### Exit Criteria

- The workspace compiles and tests without `loro-internal`.
- Documentation references only the merged crate structure.
- The migration guide is ready if any compatibility break happened.

### Validation

- workspace build and test pass
- repository-wide search confirms there are no `loro-internal` references left, except historical changelog text

### Phase 6 Summary

Completed on 2026-03-06.

- Deleted `crates/loro-internal` from the workspace and the repository.
- Moved the remaining shim-owned assets into the merged crate layout:
  - version file to `crates/loro/VERSION`
  - automerge benchmark payload to `crates/bench-utils/data/automerge-paper.json.gz`
  - compatibility tests to `crates/loro/tests/internal_*` plus `internal_compat_src/**`
- Updated the merged crate and repo tooling to use the new single-crate locations:
  - `crates/loro/src/internal/mod.rs` now reads `crates/loro/VERSION`
  - `scripts/cargo-release.ts` now syncs `crates/loro/VERSION`
  - `crates/bench-utils/src/lib.rs` now reads the relocated benchmark payload
- Updated docs/spec references to the merged `crates/loro/src/internal/**` layout and removed live code/comments/docs references that still pointed at the deleted shim crate.
- Validation passed:
  - `cargo test -p loro`
  - `cargo check --workspace`
  - `rg -n "loro-internal|loro_internal::" . -g '!target'`
- Residual `loro-internal` hits are intentionally historical:
  - this migration plan document
  - changelog entries
  - lockfile entries for fuzz/compatibility crates that pin older git revisions where `loro-internal` was still part of the published dependency graph

### Risks

- Deleting the shim too early
- Leaving stale internal references in docs, scripts, or examples

## Cross-Phase Risks

- Event-shape compatibility may dominate the schedule if not decided early.
- `loro-wasm` may force a broader internal support surface than desired.
- Constructor semantics may regress if the merge treats `loro-internal::LoroDoc::new()` as equivalent to the current public `loro::LoroDoc::new()`.
- Public documentation quality may regress if internal types become canonical before docs are migrated.

## Open Questions

- [x] The current merge uses a compatibility-first event story rather than a semver-major Rust event-layer cleanup.
- [x] The low-level support module exposed for `loro-wasm` is `loro::internal`.
- [ ] Which escape hatches should remain public after the merge, and which should be deprecated?
- [x] Public canonical names remain `LoroText` / `LoroList` / `LoroMap` / `LoroTree` / `LoroMovableList` as compatibility wrappers, while first-party low-level canonical names use the handler layer under `loro::internal`.
- [ ] How long should compatibility wrappers remain after the canonical APIs are introduced?

## Decision Log

- [x] 2026-03-06: Phase 0 baseline commands for `loro-internal` benches must include `--features test_utils`; without it the existing benchmark files fall back to no-op stubs.
- [x] 2026-03-06: Keep the existing `loro-internal` benches as split-architecture baselines, and add `crates/loro/benches/merge_baseline.rs` to measure public facade overhead on active subscriptions, heterogeneous reads, diff/apply-diff, and undo callbacks.
- [x] 2026-03-06: Phase 1 uses an embedded engine layout under `crates/loro/src/internal/**`, while `crates/loro-internal` becomes a forwarding shim. This removes the dependency edge first and defers facade collapse to Phases 2-4.
- [x] 2026-03-06: `inner()` remains as a compatibility escape hatch in Phase 2 and returns an internal `LoroDoc` view over the same `Arc<LoroDocInner>`. `with_oplog()` and `with_state()` remain unchanged for now; narrowing them is deferred.
- [x] 2026-03-06: Phase 3 keeps public `LoroList` / `LoroMap` / `LoroText` / `LoroTree` / `LoroMovableList` and `ValueOrContainer` as compatibility wrappers. The canonical first-party container/value surface is the handler layer re-exported from `loro::internal`.
- [x] 2026-03-06: Phase 4 adopts a compatibility-first event model. First-party crates should use the `loro::internal` event/diff/undo re-exports directly, while the public `loro::event::*` and public undo callback payloads remain compatibility bridges for now.
- [x] 2026-03-06: Phase 5 uses `loro::internal` as the single low-level support surface for `loro-wasm`; do not introduce a second wasm-only support module unless a later narrowing pass proves necessary.
- [x] 2026-03-06: Phase 6 treats plan text, changelog text, and lockfile entries for fuzz compatibility crates that pin historical git revisions as acceptable historical `loro-internal` references. No live workspace code, manifests, tests, or docs should depend on the deleted crate.

## Suggested PR Sequence

1. `test(loro): lock behavior and perf baseline`
2. `refactor(loro): import internal engine into public crate`
3. `refactor(loro): merge canonical LoroDoc semantics`
4. `refactor(loro): collapse container and value surface`
5. `refactor(loro): collapse event, diff, and undo surface`
6. `refactor(wasm): migrate first-party consumers to loro`
7. `refactor(loro): remove internal shim and finalize cleanup`

## Definition of Done

This plan is complete when all of the following are true:

- `loro` is the only core Rust crate for the implementation.
- `loro-internal` has been removed.
- First-party consumers use `loro`.
- The canonical event/value/container/doc surfaces no longer require the current facade conversion layers on hot paths.
- Public semantics remain correct and documented.
