# Release preparation and component versions

Verified against code 2026-11-24 (functional baseline `4c6daa2f`).

The public Loro API version and an internal crate's package version can differ.
`crates/loro-internal/VERSION` supplies Rust's `LORO_VERSION` constant; it tracks
the public API release, not necessarily `crates/loro-internal/Cargo.toml`'s
package version. The WASM `LORO_VERSION()` function instead reports its own
`CARGO_PKG_VERSION` in `crates/loro-wasm/src/lib.rs`.

For the 1.19.1 candidate, publish `loro` and `loro-kv-store` 1.19.1, and
`loro-internal` 1.19.2. Internal 1.19.1 was already published for the native
32-bit layout fix. Its existing version and tag must not be reused for the new
memory/KV implementation. Raise the corresponding path dependency minimums.
Leave unchanged component crate versions alone.

`scripts/cargo-release.ts` prepares one target version for its selected workspace
crates. A release with independently advanced component versions needs reviewed
component-specific bumps instead of blindly applying that single version.

`.github/workflows/publish-crates.yml` passes `RELEASE_FLAGS` from the release PR
to both `cargo release publish` and `cargo release tag`. Explicitly exclude every
workspace package outside the selected release set, including private tools:
`publish = false` does not alone prevent cargo-release from tagging a package.
Do not force-update existing tags to recover from a tag push rejection.

Binding manifests can be prepared before the Rust upload, but their final registry
lockfiles require the published crate checksums. Validate local source overrides
only in isolated candidate copies, then resolve and retest registry dependencies
after the Rust release. The public publish and tag steps are separate from this
preparation stage.

For direct `loro-kv-store` consumers, incremental `flush()` may retain several L0
SSTables; `export_all()` or full compaction produces the consolidated export.
See [the memory-budget contract](memory-budget.md) for the reclamation and flush
invariants.
