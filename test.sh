Mode="$*"

if [ $Mode == "wasm" ]; then
    cargo test --features test_utils wasm -- --show-output
else
    cargo test --features test_utils -- --show-output
fi
