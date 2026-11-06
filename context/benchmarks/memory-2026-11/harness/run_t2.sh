#!/bin/bash
cd /root/spikes/loro-proto; export CARGO_BUILD_JOBS=1
echo "--- integration: stock"
timeout 1200 cargo test -p loro --tests 2>&1 | grep -E "^test result|^test .*FAILED|^error|Running " | cut -c1-110
echo "--- integration: cap=8"
LORO_PROTO_BLOCK_CAP=8 timeout 1200 cargo test -p loro --tests 2>&1 | grep -E "^test result|^test .*FAILED|^error" | cut -c1-110
echo T2_DONE
