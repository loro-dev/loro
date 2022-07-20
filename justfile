build:
  cargo build

test:
  cargo nextest run

# test without proptest
test-fast:
  RUSTFLAGS='--cfg no_proptest' cargo nextest run
