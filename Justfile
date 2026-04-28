default:
    @just --list

build:
    cargo build

test:
    cargo nextest run --features=test_utils,jsonpath --no-fail-fast
    cargo test --doc

check:
    cargo clippy --all-features -- -Dwarnings

test-wasm:
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/loro-wasm
    pnpm i
    just build-dev

release-wasm:
    #!/usr/bin/env bash
    set -euo pipefail
    deno run -A ./scripts/sync-loro-version.ts
    cd crates/loro-wasm
    pnpm i
    just build-release

ci-release-wasm-version:
    pnpm changeset version
    deno run -A ./scripts/sync-loro-version.ts

ci-release-wasm-publish:
    just release-wasm
    pnpm changeset publish --access public

test-loom:
    LOOM_MAX_PREEMPTIONS=2 RUSTFLAGS='--cfg loom' cargo test -p loro --test multi_thread_test --release

test-loom-reproduce:
    LOOM_MAX_PREEMPTIONS=2 LOOM_LOG=info LOOM_LOCATION=1 LOOM_CHECKPOINT_INTERVAL=1 LOOM_CHECKPOINT_FILE=loom_test.json RUSTFLAGS='--cfg loom' cargo test -p loro --test multi_thread_test --release

coverage:
    mkdir -p coverage
    cargo llvm-cov nextest --features test_utils,jsonpath --lcov > coverage/lcov-nextest.info
    cargo llvm-cov report
