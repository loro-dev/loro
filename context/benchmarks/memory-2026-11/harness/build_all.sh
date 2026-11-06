#!/bin/bash
# Fails hard: a stale binary must not be measured.
set -euo pipefail
cd /root/spikes/loro-mem; export CARGO_BUILD_JOBS=2
b() { d=$1; f=$2; sfx=$3; (cd $d; cargo build --release --features "$f" > ../out/build.$sfx.log 2>&1 || { tail -30 ../out/build.$sfx.log; exit 1; }; for x in memprof multidoc; do cp target/release/$x ../bin/$x.$sfx; done); }
b proto proto proto
b proto proto,dhat_heap proto.dhat
b main main_api main
b main main_api,dhat_heap main.dhat
b v117 "" v117
date +%s > bin/BUILD_STAMP
echo BUILD_ALL_OK
