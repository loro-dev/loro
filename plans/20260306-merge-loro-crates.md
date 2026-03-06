# Plan: Merge `crates/loro` and `crates/loro-internal` into One Crate

Date: 2026-03-06
Status: Draft
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
| 0 | Lock behavior and perf baseline | Not Started | None | Baseline tests and benchmark numbers |
| 1 | Import engine into `loro` | Not Started | Phase 0 | `loro` owns implementation modules |
| 2 | Merge canonical `LoroDoc` semantics | Not Started | Phase 1 | One canonical document type |
| 3 | Collapse container and value surface | Not Started | Phase 2 | One canonical container/value layer |
| 4 | Collapse event, diff, and undo surface | Not Started | Phase 3 | One canonical event/diff/undo layer |
| 5 | Migrate `loro-wasm` and first-party consumers | Not Started | Phase 4 | No first-party dependency on `loro-internal` |
| 6 | Remove shim and finalize cleanup | Not Started | Phase 5 | Single-crate steady state |

## Phase 0: Lock Behavior and Performance Baseline

Status: Not Started

### Objective

Freeze the current public behavior and record baseline performance before changing crate boundaries.

### Why This Phase Exists

Without a baseline, later phases can accidentally change semantics while still compiling cleanly. This phase turns the current behavior into an explicit contract.

### Primary Scope

- Public Rust behavior exercised through `crates/loro`
- Engine correctness currently covered by `crates/loro-internal`
- Performance-sensitive paths affected by facade-to-engine conversions

### Work Items

- [ ] Inventory the public semantics currently provided only by `crates/loro`.
- [ ] Treat `crates/loro/tests` as the public compatibility suite.
- [ ] Identify the minimum subset of `crates/loro-internal/tests` that must remain green throughout the migration.
- [ ] Add or confirm benchmark coverage for:
  - active subscriptions
  - heterogeneous reads
  - diff/apply-diff paths
  - undo callbacks
- [ ] Record baseline commands and store baseline numbers in the PR or a linked artifact.

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
- `cargo bench -p loro-internal event`
- `cargo bench -p loro-internal pending`
- `cargo bench -p loro-internal list`

### Risks

- Missing a public semantic edge case and treating it as implementation detail later
- Measuring the wrong paths and optimizing the wrong layer

## Phase 1: Import the Engine into `loro`

Status: Not Started

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

- [ ] Create an internal module tree inside `crates/loro` to host the current engine implementation.
- [ ] Merge the dependency sets from `crates/loro` and `crates/loro-internal`.
- [ ] Merge feature flags while preserving the public `loro` feature contract.
- [ ] Make `crates/loro` compile against the local engine modules instead of the path dependency on `loro-internal`.
- [ ] Convert `crates/loro-internal` into a temporary compatibility shim that re-exports from `loro`.
- [ ] Keep the workspace buildable through the shim while downstream consumers are still migrating.

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

### Risks

- Import cycles or accidental feature drift
- Moving too much public surface into the root of `loro`

## Phase 2: Merge Canonical `LoroDoc` Semantics

Status: Not Started

### Objective

Remove the outer `LoroDoc { doc: InnerLoroDoc }` split and make one canonical `LoroDoc` type own the merged behavior.

### Primary Scope

- document constructors
- auto-commit behavior
- `fork`, `fork_at`, `from_snapshot`
- container `doc()` behavior
- public escape hatches such as `inner()`, `with_oplog`, and `with_state`

### Work Items

- [ ] Move the current public constructor semantics into the canonical merged `LoroDoc`.
- [ ] Preserve current auto-commit behavior for:
  - `new()`
  - `from_snapshot()`
  - `fork_at()`
  - `doc()` returned from attached containers
- [ ] Decide whether `inner()` survives, is deprecated, or is replaced by a narrower API.
- [ ] Decide the long-term shape of `with_oplog` and `with_state`.
- [ ] Ensure first-party constructors such as those in `loro-wasm` keep the same behavior.

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

### Risks

- Silent drift between manual-commit and auto-commit behavior
- Accidentally widening low-level lock or state APIs into permanent public contract

## Phase 3: Collapse the Container and Value Surface

Status: Not Started

### Objective

Remove the duplicated container and value layers where the facade currently wraps internal handlers and value enums.

### Primary Scope

- `LoroList`, `LoroMap`, `LoroText`, `LoroTree`, `LoroMovableList`, `LoroCounter`
- `Container`
- `ValueOrContainer`
- `ContainerTrait`
- handler-to-container and value-to-value conversion paths

### Work Items

- [ ] Choose the canonical naming strategy for container handles.
- [ ] Decide whether to keep public names such as `LoroText` as aliases, renamed canonical types, or compatibility wrappers.
- [ ] Collapse `Container` and the internal handler enum into one canonical representation where practical.
- [ ] Collapse `ValueOrContainer` and `ValueOrHandler` into one canonical representation where practical.
- [ ] Remove or narrow `ContainerTrait` if it only exists to bridge two type layers.
- [ ] Eliminate conversion-heavy read paths from:
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

### Risks

- Public type inference changes
- Loss of ergonomic names or public docs if aliasing is chosen poorly

## Phase 4: Collapse the Event, Diff, and Undo Surface

Status: Not Started

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

Before implementation starts, choose one of these approaches and record it in the Decision Log:

- Compatibility-first: introduce a canonical borrowed or raw event API, keep the current owned event API as a compatibility layer for one or more releases.
- Break-now: replace the old event shape immediately in a semver-major change.

### Work Items

- [ ] Select the canonical event model.
- [ ] Select the canonical diff model.
- [ ] Decide whether old `subscribe` remains temporarily as a compatibility wrapper.
- [ ] Update `UndoManager` callback payloads to use the canonical event or diff types.
- [ ] Remove first-party hot-path dependence on facade-only event reconstruction.
- [ ] Re-run the active-subscription benchmark and compare against the Phase 0 baseline.

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

### Risks

- Public lifetime complexity if a borrowed event API becomes public
- Compatibility overhead if the old and new event APIs must coexist for too long

## Phase 5: Migrate `loro-wasm` and Other First-Party Consumers

Status: Not Started

### Objective

Remove first-party dependencies on `loro-internal` and make `loro` the only crate used by workspace consumers.

### Primary Scope

- `crates/loro-wasm`
- examples
- benches
- internal tools or support crates still depending on `loro-internal`

### Work Items

- [ ] Switch `crates/loro-wasm` imports from `loro-internal` to `loro`.
- [ ] If low-level support is still needed, expose a narrowly scoped internal support module from `loro` rather than exposing the entire engine root.
- [ ] Keep the JS pending-event flush invariant intact.
- [ ] Audit all workspace members and remove direct `loro-internal` imports.
- [ ] Update examples and benches to use the merged crate surface.

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

### Risks

- Accidentally promoting too much engine API to long-term public surface just to satisfy `loro-wasm`
- Breaking the event flush invariant while touching binding code

## Phase 6: Remove the Shim and Finalize Cleanup

Status: Not Started

### Objective

Delete the temporary compatibility crate and complete the transition to a true single-crate architecture.

### Primary Scope

- `crates/loro-internal`
- docs
- readmes
- release notes
- migration notes

### Work Items

- [ ] Delete `crates/loro-internal`.
- [ ] Remove any temporary compatibility aliases or forwarding code that no longer serves a migration purpose.
- [ ] Consolidate remaining tests, benches, and docs into the merged `loro` crate.
- [ ] Update crate documentation and repository documentation.
- [ ] Write release notes and a migration guide if any user-visible API moved or changed.

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

### Risks

- Deleting the shim too early
- Leaving stale internal references in docs, scripts, or examples

## Cross-Phase Risks

- Event-shape compatibility may dominate the schedule if not decided early.
- `loro-wasm` may force a broader internal support surface than desired.
- Constructor semantics may regress if the merge treats `loro-internal::LoroDoc::new()` as equivalent to the current public `loro::LoroDoc::new()`.
- Public documentation quality may regress if internal types become canonical before docs are migrated.

## Open Questions

- [ ] Will the merge allow a semver-major Rust API cleanup for the event layer?
- [ ] What should be the exact name of any hidden internal support module exposed for `loro-wasm`?
- [ ] Which escape hatches should remain public after the merge, and which should be deprecated?
- [ ] Should canonical container type names remain `LoroText` / `LoroList` / `LoroMap`, or should they be renamed around the current internal handler names?
- [ ] How long should compatibility wrappers remain after the canonical APIs are introduced?

## Decision Log

- [ ] No decisions recorded yet.

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
