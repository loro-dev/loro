build:
  cargo build

test *FLAGS:
  RUST_BACKTRACE=full cargo nextest run {{FLAGS}}

test-all:
  cargo nextest run &
  just quickfuzz
  
quickfuzz:
  cd crates/loro-core && just quick-fuzz

check:
  cargo clippy

check-unsafe:
  env RUSTFLAGS="-Funsafe-code --cap-lints=warn" cargo check

fix:
  cargo clippy --fix

deny:
  cargo deny check

vet:
  cargo vet

bench-rle:
  cd crates/rle
  cargo build --release --examples
  cd ../..
  hyperfine --warmup=3 "./target/release/examples/string_tree_bench"


