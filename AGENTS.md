# Repository Guidelines

## Project Structure & Module Organization
Loro is a Rust workspace rooted at `Cargo.toml`, with domain-specific crates under `crates/`. `crates/loro` hosts the core CRDT engine plus integration tests in `crates/loro/tests`. Supporting crates provide shared primitives (`crates/loro-common`, `crates/rle`), storage layers (`crates/kv-store`), WASM bindings (`crates/loro-wasm`), and benchmarking or utilities. TypeScript-facing assets and demos for the WASM build live in `crates/loro-wasm/tests` and `crates/loro-wasm/web`. Additional documentation sits in `docs/`, runnable samples in `examples/`, and automation scripts in `scripts/`.

## Toolchain Setup
Use the latest stable Rust toolchain with the `wasm32-unknown-unknown` target installed. Install `cargo-nextest`, `cargo-llvm-cov`, `cargo-fuzz`, `wasm-bindgen-cli@0.2.100`, `wasm-opt`, and `wasm-snip` to match CI expectations. Node 18+, pnpm, and Deno are required for packaging, version sync scripts, and doc tasks.

## Build, Test, and Development Commands
- `pnpm build` – compile every crate via `cargo build`.
- `pnpm check` – run Clippy on all features; auto-fix with `pnpm fix` when possible.
- `pnpm test` – execute the Rust suite through Nextest and doctests.
- `pnpm test-wasm` – rebuild the WASM package and run its TypeScript tests.
- `pnpm test-loom` – stress concurrency-sensitive changes with Loom.
- `pnpm coverage` – write `coverage/lcov-nextest.info` using `cargo llvm-cov`.
Run these from the repository root; keep Loom retries and environment overrides local to reproduction efforts.

## Testing Guidelines
Add unit tests alongside the impacted crate (`crates/*/tests` or inline `mod tests`). Cover new CRDT behaviors with deterministic Nextest cases; concurrency paths should also gain Loom scenarios when feasible. For WASM bindings, extend the Vitest/Deno suites under `crates/loro-wasm/tests` or `deno_tests`. Always run `pnpm test` before opening a PR, and include `pnpm test-wasm` whenever touching JS/WASM surfaces.

## Commit & Pull Request Guidelines
Follow the conventional commit prefixes used in history (`feat:`, `fix:`, `chore:`). Include concise scopes when touching specific crates (e.g., `feat(loro-wasm): …`). PR descriptions should summarize intent, list key commands executed, link related issues, and call out performance or compatibility impacts. Attach screenshots or short clips for UI-oriented examples under `examples/`. Request maintainer review only after CI is green.
