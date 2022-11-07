build:
  cargo build

test *FLAGS:
  RUST_BACKTRACE=full cargo nextest run --features=fuzzing {{FLAGS}}

test-all:
  cargo nextest run --features=fuzzing &
  just _quickfuzz

test-prop:
  cargo nextest run --features=proptest
  
_quickfuzz:
  cd crates/loro-core && just quick-fuzz

check:
  cargo clippy

check-unsafe:
  env RUSTFLAGS="-Funsafe-code --cap-lints=warn" cargo check

fix *FLAGS:
  cargo clippy --fix --features=fuzzing {{FLAGS}}

deny:
  cargo deny check

vet:
  cargo vet

bench-rle:
  cd crates/rle
  cargo build --release --examples
  cd ../..
  hyperfine --warmup=3 "./target/release/examples/string_tree_bench"


