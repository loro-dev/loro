#!/bin/bash
cd /root/spikes/loro-mem
P=bin/memprof.proto
$P data/d1m.snap sizes > /dev/null || { echo STALE; exit 1; }
c() { cut -c1-92; }
echo "### sanity near-head vv vs slow path"
for D in d1m dlong; do LORO_PROTO_CHECK_VV=1 timeout 200 $P data/$D.snap load open0 dropshared sub detach_edit w_detached:1,100 w_detached:100,50 2>&1 | grep -E "panicked|mismatch|peak_rss"; done
for D in d28m dlong; do
 for ops in "prep_upd import_upd" "sub prep_upd import_upd" "prep_upd local_edit import_upd" "f2v_head" "detach_edit checkout_back:1"; do
  echo "### round2 $D | $ops"; timeout 120 $P data/$D.snap load open0 dropshared $ops 2>&1 | grep -E "^(open0|import_upd|f2v_head|checkout_back|peak)|panicked" | c
 done
 for cap in 256 max; do
  if [ $cap = max ]; then unset LORO_PROTO_BLOCK_CAP; else export LORO_PROTO_BLOCK_CAP=$cap; fi
  echo "### detached loop $D cap=$cap"; timeout 200 $P data/$D.snap load open0 dropshared sub detach_edit w_detached:1,40 w_detached:100,40 trim 2>&1 | grep -E "^(w_|trim|peak|    n=)|panicked" | c
  echo "### shadow loop $D cap=$cap"; timeout 200 $P data/$D.snap load open0 shadow0 dropshared sub w_shadow:1,40 w_shadow:100,40 trim 2>&1 | grep -E "^(w_|trim|peak|    n=)|panicked" | c
  echo "### export $D cap=$cap"; timeout 200 $P data/$D.snap load open0 dropshared export_upd trim 2>&1 | grep -E "^(export|trim|peak)|panicked" | c
 done
 unset LORO_PROTO_BLOCK_CAP
done
for spec in "new 2000" "open0 2000 tiny" "open0 2000 s10" "open0write 2000 s10" "open0 1000 s100"; do bin/multidoc.proto $spec | cut -c1-110; done
echo "### vv A/B: fresh-peer stale writes, d1m, 800 writes"
for e in "LORO_PROTO_VV_COW=1 LORO_PROTO_NO_NEAR=1" "LORO_PROTO_NO_NEAR=1" "X=1"; do echo "-- $e"; env $e timeout 250 $P data/d1m.snap load open0 dropshared sub detach_edit w_detached:1,800 stats 2>&1 | grep -E "^(w_|peak|    (n=|ops))|panicked" | c; done
echo "-- main"; timeout 250 bin/memprof.main data/d1m.snap load open0 dropshared sub detach_edit w_detached:1,800 stats 2>&1 | grep -E "^(w_|peak|    (n=|ops))" | c
echo "### shallow main, fixed single-head frontier"
for back in 20000 2000000; do
 echo "-- back=$back read/append"; timeout 200 bin/memprof.main data/d28m.snap load open0 dropshared shallow_at:$back open0 dropshared len read_tail:20 append:1 trim 2>&1 | grep -vE "^(stage|load|dropshared)" | c
 echo "-- back=$back stale loop"; timeout 200 bin/memprof.main data/d28m.snap load open0 dropshared shallow_at:$back open0 dropshared sub detach_edit w_detached:1,40 trim 2>&1 | grep -E "^(open0|w_|trim|peak|    (n=|shallow))|panicked" | c
done
echo "### state-only zero-copy"; timeout 100 bin/memprof.main data/d28m.snap load open0 dropshared state_only open0 dropshared len read_tail:20 append:1 trim 2>&1 | grep -vE "^(stage|load|dropshared)" | c
echo "### dhat dlong tracker"
DHAT_OUT=out/dhat/dlong_track.json timeout 280 bin/memprof.main.dhat data/dlong.snap load open0 dropshared sub detach_edit checkout_back:40000 > out/dhat/dlong_track.log 2>&1; echo rc=$?
DHAT_OUT=out/dhat/dlong_track2.json timeout 280 bin/memprof.main.dhat data/dlong.snap load open0 dropshared sub detach_edit checkout_back:40000 free_dc latest free_dc free_hc > out/dhat/dlong_track2.log 2>&1; echo rc=$?
DHAT_OUT=out/dhat/dlong_p_export.json timeout 280 bin/memprof.proto.dhat data/dlong.snap load open0 dropshared export_upd > out/dhat/dlong_p_export.log 2>&1; echo rc=$?
echo DONE
