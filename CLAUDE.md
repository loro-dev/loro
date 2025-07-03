# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Build Commands
- `cargo build` - Build the Rust project
- `pnpm release-wasm` - Build WASM release (includes version sync)
- `pnpm test-wasm` - Build WASM development version

### Test Commands
- `cargo nextest run --features=test_utils,jsonpath --no-fail-fast && cargo test --doc` - Run all tests
- `pnpm test-all` - Run all tests including WASM and loom
- `pnpm test-loom` - Run concurrency tests with loom
- `pnpm run-fuzz-corpus` - Run fuzz tests

### Lint and Check Commands
- `cargo clippy --all-features -- -Dwarnings` - Run clippy with warnings as errors
- `cargo hack check --each-feature` - Check each feature combination
- `cargo clippy --fix --features=test_utils` - Auto-fix clippy warnings

### Coverage
- `pnpm coverage` - Generate code coverage report using llvm-cov

## Architecture

Loro is a CRDT (Conflict-free Replicated Data Types) library for building local-first collaborative applications.

### Core Crate Structure
- **`loro`** - Public API facade providing clean interfaces
- **`loro-internal`** - Core CRDT implementation, not meant for direct use
- **`loro-common`** - Shared types and traits used across crates
- **`loro-wasm`** - WebAssembly bindings for JavaScript/TypeScript
- **`loro-ffi`** - Foreign Function Interface for other languages

### Key Architectural Components

**OpLog and DocState Separation**: Loro separates operation history (OpLog) from current state (DocState). This enables:
- Time travel through history
- Efficient state checkouts
- Memory optimization through shallow snapshots

**Container System**: All data structures (text, list, map, tree) are containers with:
- Unique container IDs (cid)
- Parent-child relationships
- Type-specific operations

**Event System**: 
- DiffCalculator computes changes between versions with multiple modes (Checkout, Import, Linear)
- Events propagate through subscriptions for reactive updates
- Supports both local and remote change events

**Memory Management**:
- SharedArena for centralized memory allocation
- String interning and value deduplication
- Run-length encoding for operation compression

### CRDT Data Structures
- **Text/RichText**: B-tree based with Unicode/UTF-16/byte indexing, supports styles
- **List**: Positional list with move operations
- **Map**: Last-write-wins register semantics
- **Tree**: Movable tree with fractional indexing
- **Counter**: Convergent counter (optional feature)

### Testing Philosophy
- Extensive property-based testing with proptest
- Fuzzing for robustness
- Loom for concurrency correctness
- Integration tests for cross-language bindings