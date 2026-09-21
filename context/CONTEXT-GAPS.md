# Context Discoverability Gaps (backlog)

Append a line when you discovered something important the hard way but could not
fix the docs in that change.

Format:
`YYYY-MM-DD | <question an agent would ask> | <answer + file anchors> | why it was hard | suggested home`
2026-09-21 | How do I test that loro.js output imports into Rust/loro-crdt? | `loro-js/scripts/rewrite-rust-fixture.mjs` (`pnpm -C loro-js fixtures:rewrite`) writes `loro-js/tests/fixtures/rust/*.ts.blob`, consumed by `crates/loro/tests/loro_js_interop.rs`; loro.js state encoders live in `loro-js/src/runtime/document.ts` (`#containerState`) and must follow `docs/encoding-container-states.md` (tree sibling order: loro-dev/loro#1088) | root AGENTS only links loro.js performance context; the interop path is mentioned only in `loro-js/README.md` | a `loro-js/AGENTS.md` or a loro.js interop context article
