#!/bin/bash
# Prototype validity: stock tests, then again with a tiny block cap to stress eviction.
cd /root/spikes/loro-proto; export CARGO_BUILD_JOBS=2
cargo test -p loro-internal --lib -p loro-kv-store 2>&1 | tail -4
echo "--- integration: loro"
timeout 1500 cargo test -p loro --tests 2>&1 | grep -E "^test result|FAILED|failed|panicked at" | sort | uniq -c | sort -rn | head -14
echo "--- cap=8"
LORO_PROTO_BLOCK_CAP=8 timeout 1200 cargo test -p loro-internal --lib 2>&1 | grep -E "^test result|^test .*FAILED|panicked at" | sort | uniq -c | sort -rn | head -14
LORO_PROTO_BLOCK_CAP=8 timeout 1200 cargo test -p loro --tests 2>&1 | grep -E "^test result|^test .*FAILED" | sort | uniq -c | sort -rn | head -14
echo TESTS_DONE
