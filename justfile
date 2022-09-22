build:
  cargo build

test *FLAGS:
  RUST_BACKTRACE=full cargo nextest run {{FLAGS}}

# test with proptest
test-prop *FLAGS:
  RUST_BACKTRACE=full RUSTFLAGS='--cfg proptest' cargo nextest run {{FLAGS}}

# test with slower proptest
test-slowprop *FLAGS:
  RUST_BACKTRACE=full RUSTFLAGS='--cfg slow_proptest' cargo nextest run {{FLAGS}}

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


