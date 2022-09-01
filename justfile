build:
  cargo build

test *FLAGS:
  RUST_BACKTRACE=full cargo nextest run {{FLAGS}}

# test without proptest
test-fast *FLAGS:
  RUSTFLAGS='--cfg no_prop_test' cargo nextest run {{FLAGS}}

# test with slower proptest
test-slow *FLAGS:
  RUSTFLAGS='--cfg slow_proptest' cargo nextest run {{FLAGS}}

check-unsafe:
  env RUSTFLAGS="-Funsafe-code --cap-lints=warn" cargo check

fix:
  cargo clippy --fix

deny:
  cargo deny check

crev:
  cargo crev crate check

bench-rle:
  cd crates/rle
  cargo build --release --examples
  cd ../..
  hyperfine --warmup=3 "./target/release/examples/string_tree_bench"


