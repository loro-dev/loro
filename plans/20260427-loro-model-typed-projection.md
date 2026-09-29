# Plan: Typed `loro_model` Layer with Projection and Reconcile

Date: 2026-04-27
Status: Draft
Primary package targets: new `loro-model` runtime crate, optional `loro-model-derive` proc-macro crate
Related issue: <https://github.com/loro-dev/loro/issues/888>
Compatibility stance: Additive and opt-in. Do not change existing `loro` public APIs in the first iteration.

## How to Use This Document

- Update each phase status as work progresses: `Not Started`, `In Progress`, `Blocked`, or `Done`.
- Each PR should update the relevant checklist items and validation notes.
- A phase is only `Done` when all exit criteria are satisfied.
- If a design decision changes the public API shape, update the "Decision Log" section first.

## Summary

Issue #888 asks whether Loro can provide autosurgeon-like automatic conversion between Rust structs and Loro documents.

The recommended design is not to make arbitrary user structs the primary live model. Instead, Loro should provide a first-party typed model layer:

- `LoroModel`
- `loro_model::LoroVec`
- `loro_model::LoroMap`
- typed scalar/text/counter wrappers

These model types are the ergonomic, high-fidelity path for reading and writing collaborative state. They preserve Loro-specific semantics such as container identity, text CRDT behavior, keyed list matching, event routing, and incremental updates.

For interoperability with application-owned data types, the model layer also provides:

- `read_as`: project or materialize model data into user structs and standard Rust collections.
- `update_from`: reconcile user structs and standard Rust collections back into the model while preserving existing Loro containers where possible.

This gives users both a convenient typed model and a boundary conversion API, without forcing every application to use Loro model types everywhere.

## End-to-End Example

This section is the most important API fixture. It should stay near the top of the plan because it describes the intended developer experience and exposes whether the design is technically feasible.

The derive macro should let users define one Rust shape and get:

- DTO-style projection and update support through `FromLoroModel` and `ToLoroModel`.
- A generated concrete model wrapper such as `ProjectModel`.
- Generated field accessors for efficient local updates.
- Stable keyed-list accessors when a field is marked with `#[loro(key)]`.

The generated concrete wrapper is important. Rust cannot let a user crate add inherent methods directly to a foreign generic type such as `loro_model::LoroModel<Project>`. A derive macro can, however, generate a local wrapper type:

```rust
// Generated shape, conceptually:
pub struct ProjectModel {
    inner: loro_model::LoroModel<Project>,
}
```

That wrapper can have normal inherent methods such as `todos_mut()`, `notes_mut()`, and `read_as()`.

### Define the Data Shape

```rust
use loro::{ExportMode, LoroDoc};
use loro_model::{FromLoroModel, LoroModel, ToLoroModel};

#[derive(Clone, Debug, PartialEq, LoroModel, FromLoroModel, ToLoroModel)]
#[loro(model = ProjectModel)]
struct Project {
    title: String,

    #[loro(vec, key = "id")]
    todos: Vec<Todo>,

    #[loro(text)]
    notes: String,
}

#[derive(Clone, Debug, PartialEq, LoroModel, FromLoroModel, ToLoroModel)]
#[loro(model = TodoModel)]
struct Todo {
    #[loro(key)]
    id: String,
    title: String,
    done: bool,
}
```

The plain structs remain useful as application DTOs. The generated model wrappers are the preferred live model for collaborative reads and writes.

Suggested mapping:

| Field | Loro model backing |
| --- | --- |
| `Project.title: String` | scalar string value |
| `Project.todos: Vec<Todo>` | `LoroVec<TodoModel>` backed by `LoroList` |
| `Project.notes: #[loro(text)] String` | `LoroTextValue` backed by `LoroText` |
| `Todo.id: #[loro(key)] String` | scalar string plus keyed-list identity |
| `Todo.done: bool` | scalar bool value |

### Full Reconcile and Full Hydrate

```rust
let doc = LoroDoc::new();
let mut project = ProjectModel::attach(doc.get_map("project"))?;

let initial = Project {
    title: "Issue 888 design".to_string(),
    todos: vec![
        Todo {
            id: "todo-1".to_string(),
            title: "Write model plan".to_string(),
            done: false,
        },
        Todo {
            id: "todo-2".to_string(),
            title: "Validate API examples".to_string(),
            done: false,
        },
    ],
    notes: "Design notes\n".to_string(),
};

// Full boundary write from a plain Rust value into the Loro-backed model.
// This reconciles structure and preserves existing containers when possible.
project.update_from(&initial)?;

// Full boundary read from the Loro-backed model into a plain Rust value.
// This allocates an owned Project and is O(output size).
let hydrated: Project = project.read_as()?;
assert_eq!(hydrated, initial);
```

### Partial Local Update Through Generated Model Methods

```rust
// Local scalar update. This writes one map key and updates the model cache.
project.title_mut().set("Typed model plan")?;

// Keyed list lookup avoids treating all following items as changed when the
// list changes near the front.
let todo = project.todos_mut().by_key_mut("todo-1")?;
todo.title_mut().set("Write the typed model plan")?;
todo.done_mut().set(true)?;

// Text fields use text CRDT operations rather than replacing a scalar string.
project.notes_mut().splice(0, 0, "Decision log\n")?;
```

The intended cost of these operations is proportional to the edited path and edited value, not the whole project document.

### Import Updates and Refresh the Model

```rust
let alice = LoroDoc::new();
let mut alice_project = ProjectModel::attach(alice.get_map("project"))?;
alice_project.update_from(&initial)?;

let bob = LoroDoc::new();
bob.import(&alice.export(ExportMode::Snapshot)?)?;
let mut bob_project = ProjectModel::attach(bob.get_map("project"))?;

let bob_seen = bob.oplog_vv();

alice_project
    .todos_mut()
    .by_key_mut("todo-1")?
    .done_mut()
    .set(true)?;

let update = alice.export(ExportMode::updates(&bob_seen))?;
bob.import(&update)?;

// Explicit refresh path for the MVP. Internally this can compute:
// doc.diff(cached_frontiers, doc.state_frontiers())
// and patch only affected model nodes through the ContainerID route table.
bob_project.pull()?;

let todo = bob_project.todos().by_key("todo-1")?;
assert_eq!(todo.done().get(), true);
```

An automatic subscription-based mode can be layered on later:

```rust
let _sync = bob_project.auto_pull()?;
```

The explicit `pull()` API is still useful because it makes hidden work visible and gives applications control over when model caches are refreshed.

### Subscribe to a Model Path

```rust
let _sub = bob_project
    .todos()
    .by_key("todo-1")?
    .title()
    .subscribe(|change| {
        tracing::info!(
            old = ?change.old(),
            new = ?change.new(),
            "todo title changed"
        );
    })?;
```

The subscription should be implemented on top of the model route table and Loro container diffs. A keyed path such as `todos.by_key("todo-1").title` should remain stable when list indices shift.

### Read a Subtree as a Custom Struct

```rust
#[derive(Debug, FromLoroModel)]
struct TodoSummary {
    id: String,
    title: String,
}

let summaries: Vec<TodoSummary> = bob_project.todos().read_as()?;
```

This is a projection boundary. It should be easy and reliable, but it is not the incremental hot path. Its cost is proportional to the projected output.

### Example Requirements

The design should be considered healthy only if the example above is implementable with these properties:

- Full `update_from` and `read_as` work for plain Rust structs.
- Direct generated setters update Loro and the model cache without scanning the full model.
- `pull()` after import patches from Loro diffs instead of rehydrating the full model.
- Model-path subscriptions are stable for keyed list entries.
- `todos().read_as::<Vec<TodoSummary>>()` works for application-specific projected structs.
- The generated wrapper avoids Rust's inherent-impl limitation on foreign generic types.

## Background

Autosurgeon maps Rust data types to Automerge documents using two primary concepts:

- `Reconcile`: update a CRDT document to match a Rust value.
- `Hydrate`: build a Rust value from a CRDT document.

That is useful, but a direct clone of that design would leave two problems for Loro:

1. A stateless `reconcile(&plain_struct, &doc)` cannot know which fields changed unless it scans the struct or compares old and new values.
2. A stateless `hydrate::<PlainStruct>(&doc)` constructs a fresh Rust object each time, so even tiny remote edits can require materializing large user-owned structures.

Loro already exposes container-level diffs and subscriptions. That makes a more incremental design possible if the live representation is a model object that keeps binding metadata and cached state.

## Goals

- Provide a first-party typed model layer for common Loro document shapes.
- Make direct model operations the best user experience for frequent reads and writes.
- Preserve Loro CRDT semantics instead of collapsing everything into plain JSON-like values.
- Allow users to project model data into custom structs and common Rust collections.
- Allow users to reconcile custom structs and common Rust collections back into the model.
- Keep projection and reconcile fallible with path-rich errors.
- Support efficient incremental updates when users stay on the model path.
- Keep the first implementation additive and independent from existing `loro` APIs.

## Non-Goals

- Replacing existing `LoroDoc`, `LoroMap`, `LoroList`, or `LoroText` APIs.
- Making arbitrary plain Rust structs automatically incremental without wrappers or generated accessors.
- Guaranteeing `O(changed)` projection into plain `Vec` or `HashMap`; materializing plain output is at least proportional to output size.
- Making serde the core abstraction. Serde support can be added later as an adapter, but this design needs Loro-specific container semantics.
- Supporting every Loro container type in the MVP. Tree and advanced rich-text metadata can be added after map/list/text/counter are stable.

## Design Principles

- Model types are the live source of truth.
- Projection is a boundary operation, not the hot path.
- Reconcile from plain data should preserve existing container identity whenever possible.
- Invalid document data should return `Err`, not panic.
- Internal invariant violations inside the model binding should fail fast rather than silently producing incorrect data.
- Trait bounds should be layered. `LoroVec<T>` should not require all possible capabilities from `T` up front.
- Use Loro events and container IDs for incremental updates instead of scanning the full document after every change.

## API Shape Summary

The user-facing method traits should be implemented directly by model handles and model containers:

```rust
pub trait ReadAs {
    fn read_as<T>(&self) -> Result<T, ReadError>
    where
        T: FromLoroModel;
}

pub trait UpdateFrom {
    fn update_from<T>(&mut self, value: &T) -> Result<(), UpdateError>
    where
        T: ToLoroModel;
}
```

These traits should be implemented for generated model wrappers, `LoroModel<T>`, `LoroVec<T>`, and `loro_model::LoroMap<K, V>` where applicable.

Application-owned data types implement the conversion traits:

```rust
pub trait FromLoroModel: Sized {
    fn from_loro_model(node: LoroNodeRef<'_>) -> Result<Self, ReadError>;
}

pub trait ToLoroModel {
    fn update_loro_model(&self, target: LoroNodeMut<'_>) -> Result<(), UpdateError>;
}
```

This separation keeps the call sites simple:

```rust
let dto: Project = project.read_as()?;
project.update_from(&dto)?;
let summaries: Vec<TodoSummary> = project.todos().read_as()?;
```

while keeping trait responsibilities clear:

- Model handles know how to expose a readable or writable Loro node.
- User structs and collections know how to read from or update that node.
- Generated model wrappers provide schema-specific local update methods.

## Core Types

### `LoroModel`

`LoroModel<T>` or a generated concrete model type represents a root typed model attached to a Loro container.

Responsibilities:

- Own or reference the root `LoroDoc` and root container.
- Keep the current state frontiers for cache validity.
- Route incoming diffs by `ContainerID`.
- Expose typed model fields and mutation APIs.
- Provide `read_as` and `update_from`.

### `LoroVec<T>`

`LoroVec<T>` represents an ordered collection backed by `LoroList` or optionally `LoroMovableList`.

Capabilities should be layered:

- Read/cache only: `T: LoroHydrate`
- Insert/reconcile elements: `T: LoroReconcile`
- Incremental element patching: `T: LoroPatch`
- Stable identity and keyed lookup: `T: LoroKeyed`

The type should not globally require `T: Clone`, `T: Serialize`, `T: Deserialize`, `T: PartialEq`, `Send`, or `Sync`.

Potential API:

```rust
impl<T: LoroHydrate> LoroVec<T> {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn get(&self, index: usize) -> Option<&T>;
    pub fn iter(&self) -> impl Iterator<Item = &T>;
}

impl<T: LoroHydrate + LoroReconcile> LoroVec<T> {
    pub fn push(&mut self, value: T) -> Result<(), LoroModelError>;
    pub fn insert(&mut self, index: usize, value: T) -> Result<(), LoroModelError>;
    pub fn remove(&mut self, index: usize) -> Result<T, LoroModelError>;
}

impl<T: LoroHydrate + LoroKeyed> LoroVec<T> {
    pub fn by_key(&self, key: &T::Key) -> Option<&T>;
}
```

### `loro_model::LoroMap<K, V>`

`LoroMap<K, V>` represents a typed map backed by `LoroMap`.

MVP should prioritize string-keyed maps because Loro map keys are strings:

```rust
pub type LoroStringMap<V> = LoroMap<String, V>;
```

Non-string keys can be supported through parse/format traits:

```rust
pub trait LoroMapKey: Eq + Hash + Clone {
    fn from_loro_key(key: &str) -> Result<Self, LoroModelError>;
    fn to_loro_key(&self) -> Cow<'_, str>;
}
```

### Text and Counter Wrappers

Plain `String` should default to scalar `LoroValue::String` when used in projection/reconcile.

CRDT text should be explicit:

```rust
pub struct LoroTextValue;
```

Counter should also be explicit:

```rust
pub struct LoroCounterValue;
```

This avoids ambiguity between "replace this string scalar" and "edit this collaborative text container".

## Core Traits

### `ReadAs`

Implemented by model handles and model containers. This is the trait users normally call.

```rust
pub trait ReadAs {
    fn read_as<T>(&self) -> Result<T, ReadError>
    where
        T: FromLoroModel;
}
```

### `UpdateFrom`

Implemented by writable model handles and model containers. This is the trait users normally call for boundary writes from plain Rust values.

```rust
pub trait UpdateFrom {
    fn update_from<T>(&mut self, value: &T) -> Result<(), UpdateError>
    where
        T: ToLoroModel;
}
```

### `FromLoroModel`

Projects a Loro model node into an application-owned value.

```rust
pub trait FromLoroModel: Sized {
    fn from_loro_model(node: LoroNodeRef<'_>) -> Result<Self, ReadError>;
}
```

Implementations should be provided for:

- `bool`
- integer types with checked conversions
- `f32`, `f64`
- `String`
- `Option<T>`
- `Vec<T>`
- `HashMap<String, V>`
- `FxHashMap<String, V>`
- `BTreeMap<String, V>`
- `im` or `imbl` collection types if the dependency is accepted for this crate
- model wrapper types
- derive-generated structs and enums

### `ToLoroModel`

Reconciles an application-owned value into a target Loro model node.

```rust
pub trait ToLoroModel {
    fn update_loro_model(&self, target: LoroNodeMut<'_>) -> Result<(), UpdateError>;
}
```

Implementations should preserve target container identity where possible.

### `LoroHydrate`

Internal or semi-public trait for constructing model nodes from Loro data.

```rust
pub trait LoroHydrate: Sized {
    fn hydrate(node: LoroNodeRef<'_>, ctx: &mut HydrateCtx) -> Result<Self, HydrateError>;
}
```

This is different from `FromLoroModel`: hydration creates model objects and binding metadata, while projection creates application-owned values.

### `LoroReconcile`

Internal or semi-public trait for writing model-compatible values into Loro slots.

```rust
pub trait LoroReconcile {
    fn reconcile(&self, target: LoroNodeMut<'_>, ctx: &mut ReconcileCtx) -> Result<(), UpdateError>;
}
```

### `LoroPatch`

Applies Loro diffs to an existing cached model value.

```rust
pub trait LoroPatch: Sized {
    fn apply_patch(&self, patch: PatchCtx<'_>) -> Result<Self, PatchError>;
}
```

For model-owned types, this enables immutable-style updates with structural sharing.

### `LoroKeyed`

Provides stable identity for collection elements.

```rust
pub trait LoroKeyed {
    type Key: Eq + Hash + Clone;
    fn key(&self) -> &Self::Key;
}
```

`#[loro(key)]` derive support can generate this for model types and DTO types.

## Read-As Semantics

`read_as` should:

- Walk the requested subtree and build an owned target value.
- Return a path-rich error on type mismatch, missing required fields, failed key parse, or numeric overflow.
- Treat projection as a read boundary. It should not mutate the document or model.
- Document that full projection is `O(output_size)`.

Example error:

```text
project.todos[3].metadata["title"]: expected string, found map container
```

`read_as` should not be called `cast` because:

- It can allocate.
- It can fail.
- It can recursively convert values.
- It is not a representation-level reinterpretation.

## Update-From Semantics

`update_from` should:

- Update the model and underlying Loro document to match the input value.
- Reuse existing containers when the target shape and identity match.
- Replace containers only when required by type mismatch or key mismatch.
- Use keyed reconciliation for collections when `T: LoroKeyed` or when a derive key is available.
- Return path-rich errors for invalid input that cannot be represented.

For plain `Vec<T>`, index-based reconciliation is acceptable as the default MVP. For entity lists, keyed reconciliation should be strongly recommended.

## Incremental Model Semantics

The model layer should use Loro subscriptions or explicit diff application to keep the typed cache updated.

Each attached model should maintain:

- root container id
- current frontiers
- `ContainerID -> binding node` route table
- optional `Key -> index/container` indexes for keyed vectors
- dirty operation queue for local setter-based writes

Incoming events should be routed by `ContainerID`. Only affected model nodes should be patched.

Expected complexity:

| Operation | Expected Complexity |
| --- | --- |
| Initial attach/hydrate | `O(model_subtree_size)` |
| `read_as::<T>()` | `O(output_size)` |
| `update_from(&plain)` without keys | up to `O(input_size + affected_existing_size)` |
| `update_from(&plain)` with keys | `O(input_size + matching_cost + changed_subtree_size)` |
| Direct model setter | `O(path_depth + changed_value_size)` |
| Remote event patch | `O(diff_size * route_cost + changed_subtree_size)` |

This design should avoid claiming strict `O(changed)` for plain application data. That guarantee is only realistic for direct model operations and diff-driven cache updates.

## Persistent Data Structure Strategy

Immutable or persistent structures are useful for model caches because they allow cheap snapshots and structural sharing.

Recommended approach:

- Keep persistent collection crates behind the model layer, not in the primary public API where avoidable.
- Start with internal cache abstractions that can use `im`, `imbl`, `Arc`, or custom structures later.
- Expose `LoroVec` and `LoroMap` as the stable API surface.
- Add optional projections to `im` or `imbl` collection types if useful.

Notes:

- The workspace already depends on `im` through `loro-internal`.
- `imbl` has attractive copy-on-write behavior and default thread-safe types, but adding it should be a deliberate dependency decision.
- For large collaborative text, prefer Loro text containers or a rope-like internal representation over repeatedly materializing `String`.

## Crate Layout

### Option A: Separate Runtime and Derive Crates

Recommended initial layout:

- `crates/loro-model`
- `crates/loro-model-derive`

Benefits:

- Keeps proc-macro dependencies out of the runtime crate.
- Keeps the feature additive.
- Lets `loro` optionally re-export the model layer later.

### Option B: Feature Inside `loro`

Possible later:

- `loro = { features = ["model"] }`
- `pub use loro_model::*`

This should wait until the model API is stable enough.

## MVP Scope

The MVP should include:

- `LoroModel` attached to a root `LoroMap`.
- `LoroVec<T>` backed by `LoroList`.
- `loro_model::LoroMap<String, V>` backed by `LoroMap`.
- scalar projection and reconcile for common Rust primitives.
- `String` as scalar string.
- explicit `LoroTextValue` for text container support.
- `read_as` for model to plain Rust data.
- `update_from` for plain Rust data to model.
- derive support for named structs.
- path-rich error reporting.
- tests for roundtrip, projection failure, reconcile preservation, and remote diff patching.

MVP can defer:

- enums
- tree
- movable list reorder optimization
- rich text attributes
- serde adapter
- WASM binding wrappers
- advanced collection projections

## Phase 0: API Spike and Design Fixture

Status: Not Started

### Objective

Validate the public API shape with a small non-published prototype before committing to crate structure.

### Work Items

- [ ] Create a design fixture with `Project`, `Todo`, `LoroVec<Todo>`, and `LoroTextValue`.
- [ ] Write example code for direct model operations.
- [ ] Write example code for `read_as` into DTO structs.
- [ ] Write example code for `update_from` from DTO structs.
- [ ] Identify names that feel ambiguous or misleading.

### Deliverables

- A runnable or compile-checkable example module.
- Updated naming decisions in this plan.

### Exit Criteria

- The direct model path is clearly more ergonomic than manual Loro container code.
- Projection and reconcile APIs are understandable without reading internals.
- API names are stable enough for runtime implementation.

### Validation

- `cargo check` for the prototype crate or example.

## Phase 1: Runtime Core

Status: Not Started

### Objective

Implement the runtime traits, node references, errors, and basic model containers.

### Work Items

- [ ] Add `crates/loro-model`.
- [ ] Define `LoroNodeRef` and `LoroNodeMut`.
- [ ] Define `ReadError`, `UpdateError`, `HydrateError`, and `PatchError`.
- [ ] Define user-facing `ReadAs` and `UpdateFrom` method traits.
- [ ] Define `FromLoroModel`.
- [ ] Define `ToLoroModel`.
- [ ] Define `LoroHydrate`, `LoroReconcile`, `LoroPatch`, and `LoroKeyed`.
- [ ] Implement primitive scalar conversions.
- [ ] Implement `Option<T>`.
- [ ] Implement `Vec<T>` projection and reconcile.
- [ ] Implement `HashMap<String, V>` and `FxHashMap<String, V>` projection and reconcile.
- [ ] Implement `LoroTextValue` basics.

### Deliverables

- `loro-model` builds as a standalone runtime crate.
- Basic projection and reconcile work without derive macros.

### Exit Criteria

- Manual trait implementations can model a nested map/list/text document.
- Errors include enough path context to debug schema mismatches.

### Validation

- `cargo check -p loro-model`
- `cargo test -p loro-model`

## Phase 2: `LoroVec` and `LoroMap` Model Containers

Status: Not Started

### Objective

Provide first-party model containers that users can operate on directly.

### Work Items

- [ ] Implement `LoroVec<T>` backed by `LoroList`.
- [ ] Implement `loro_model::LoroMap<String, V>` backed by `LoroMap`.
- [ ] Add read APIs: `len`, `is_empty`, `get`, `iter`.
- [ ] Add write APIs: `push`, `insert`, `remove`, map `insert`, map `remove`.
- [ ] Add `read_as` methods on model containers.
- [ ] Add `update_from` methods on model containers.
- [ ] Preserve existing child containers during reconcile when types match.
- [ ] Define how detached vs attached model containers behave.

### Deliverables

- Usable model containers for common map/list workflows.
- Tests that direct model writes update the underlying Loro document.

### Exit Criteria

- Users can write a realistic todo/list example without touching raw `LoroMap`/`LoroList`.
- Projection to `Vec`/`FxHashMap` works.
- Reconcile from `Vec`/`FxHashMap` works.

### Validation

- `cargo test -p loro-model`
- Targeted tests under `crates/loro/tests` if the crate is integrated into the workspace APIs.

## Phase 3: Incremental Binding and Event Patch

Status: Not Started

### Objective

Make attached models update from Loro diffs without rehydrating the whole subtree.

### Work Items

- [ ] Track `ContainerID -> model node` bindings.
- [ ] Track model state frontiers.
- [ ] Apply map diffs to only affected fields.
- [ ] Apply list diffs to only affected list ranges.
- [ ] Apply text diffs to `LoroTextValue`.
- [ ] Apply counter diffs if counter support is enabled.
- [ ] Decide whether event subscription is automatic or explicit.
- [ ] Add conflict behavior tests for remote updates.

### Deliverables

- `model.pull()` or subscription-driven patching API.
- Incremental tests showing a small remote update does not rehydrate unrelated branches.

### Exit Criteria

- Remote edits to a nested field patch only the affected model node.
- Projection after remote patch reflects the latest document state.
- Route table stays consistent after insert/delete.

### Validation

- `cargo test -p loro-model`
- `cargo test -p loro --test loro_rust_test` if public integration tests are added.

## Phase 4: Keyed Collections

Status: Not Started

### Objective

Support stable identity for list elements to avoid treating insertions or reorders as unrelated element replacement.

### Work Items

- [ ] Implement `LoroKeyed`.
- [ ] Add keyed lookup to `LoroVec<T>`.
- [ ] Add keyed reconcile for `Vec<T>` when element key is known.
- [ ] Maintain `Key -> index/container` index.
- [ ] Detect duplicate keys and return errors.
- [ ] Define behavior when key field changes.
- [ ] Add tests for concurrent insert/delete/update scenarios.

### Deliverables

- `LoroVec<T>::by_key`.
- Keyed reconcile that preserves element containers.

### Exit Criteria

- Inserting an item at the front does not force all subsequent keyed items to be rewritten.
- Updating one keyed item preserves sibling identities.

### Validation

- `cargo test -p loro-model keyed`

## Phase 5: Derive Macros

Status: Not Started

### Objective

Generate model/projection/reconcile implementations for user structs.

### Work Items

- [ ] Add `crates/loro-model-derive`.
- [ ] Implement derive for named structs.
- [ ] Support `#[loro(rename = "...")]`.
- [ ] Support `#[loro(default)]`.
- [ ] Support `#[loro(missing = "path")]`.
- [ ] Support `#[loro(with = "module")]`.
- [ ] Support `#[loro(text)]`.
- [ ] Support `#[loro(key)]`.
- [ ] Emit useful compile errors for unsupported shapes.
- [ ] Add trybuild-style tests if appropriate.

### Deliverables

- `#[derive(FromLoroModel, ToLoroModel)]`
- `#[derive(LoroModel)]` that generates concrete wrappers such as `ProjectModel`.

### Exit Criteria

- Common DTO structs can be projected and reconciled with minimal boilerplate.
- Derived code preserves model semantics rather than forcing JSON-like replacement.

### Validation

- `cargo test -p loro-model-derive`
- `cargo test -p loro-model`

## Phase 6: Public Integration and Documentation

Status: Not Started

### Objective

Expose the model layer in a way that is discoverable and safe for downstream users.

### Work Items

- [ ] Decide whether `loro` should re-export `loro_model` behind a feature.
- [ ] Add docs explaining model vs projection vs reconcile.
- [ ] Add examples:
  - direct model operation
  - full `update_from` and `read_as`
  - import followed by model `pull`
  - subscribe to a generated model path
  - read a model subtree as a custom struct
  - keyed list
  - text container field
- [ ] Document complexity expectations.
- [ ] Document failure modes.
- [ ] Document how model APIs interact with auto-commit.
- [ ] If exposed to WASM later, audit the pending-event flush allowlist.

### Deliverables

- Public documentation and examples.
- Optional feature-gated re-export from `loro`.

### Exit Criteria

- Users can understand the recommended path without reading design notes.
- The crate docs clearly state when projection is full-size and when direct model operations are incremental.

### Validation

- `cargo test -p loro-model --doc`
- `cargo test -p loro --doc` if re-exported
- `pnpm check` if public Rust APIs are re-exported through existing workspace checks

## Testing Strategy

### Unit Tests

- primitive projection and reconcile
- missing field errors
- type mismatch errors
- numeric overflow errors
- map key parse errors
- text scalar vs text container behavior

### Integration Tests

- roundtrip DTO -> model -> DTO
- direct model writes update `LoroDoc`
- remote Loro update patches model cache
- keyed vector insert/delete/update
- preserving child container identity during reconcile

### Regression Tests

- reconciling a plain struct should not replace unrelated text containers.
- projecting `LoroVec<LoroMap<String>>` into `Vec<FxHashMap<String, String>>` should work.
- reconciling `Vec<FxHashMap<String, String>>` back into `LoroVec<LoroMap<String>>` should preserve compatible existing maps.

### Performance Tests

- initial attach cost vs document size
- direct setter cost vs model size
- remote event patch cost vs diff size
- full projection cost vs output size
- keyed reconcile cost for large lists

## API Naming Decisions

| Concept | Preferred Name | Avoid |
| --- | --- | --- |
| model to user data | `read_as` | `cast`, `as`, `into_plain` as the only API |
| user data to model | `update_from` | `try_set`, `replace`, `assign` |
| model-owned collection | `LoroVec`, `loro_model::LoroMap` | exposing raw third-party persistent collection types |
| CRDT text field | `LoroTextValue` or explicit `#[loro(text)]` | treating every `String` as text CRDT |

## Risks

- The model layer can become too large if it tries to cover every Loro container in the first release.
- Projection may be mistaken for a cheap cast unless docs and naming are clear.
- Reconcile from plain `Vec` can still be expensive or semantically weak without keys.
- A derive-first design may hide important container identity behavior from users.
- Publicly exposing a third-party persistent collection type could make future implementation changes harder.
- Automatic subscriptions need careful lifecycle management to avoid stale bindings or hidden work.

## Open Questions

- Should generated wrapper names default to `${TypeName}Model`, require `#[loro(model = ...)]`, or support both?
- Should model containers use `Rc` by default and offer a `sync` feature for `Arc`, or default to thread-safe `Arc`?
- Should `update_from` delete unknown extra map keys by default, or preserve them unless configured?
- How should key changes be represented: delete old item and insert new item, or error by default?
- Should `LoroVec<T>` support both list and movable-list backends in the same type, or use separate `LoroMovableVec<T>`?
- How much of this should eventually be available in `loro-wasm`?

## Decision Log

- 2026-04-27: Prefer first-party model types over making plain user structs the primary live model.
- 2026-04-27: Use `read_as` for model-to-user conversion; avoid `cast`.
- 2026-04-27: Use `update_from` for user-to-model writes; avoid `try_set` because it implies replacement.
- 2026-04-27: Have model handles and model containers implement user-facing `ReadAs` and `UpdateFrom` method traits.
- 2026-04-27: Have derive generate concrete wrapper types such as `ProjectModel` so schema-specific methods are technically feasible in Rust.
- 2026-04-27: Keep projection/reconcile as boundary APIs and direct model operations as the preferred hot path.
