#!/bin/bash
cd /root/spikes/loro-mem
for b in main main.je main.mi; do
 echo "### G1 $b detached d28m"; timeout 200 bin/memprof.$b data/d28m.snap load open0 dropshared sub detach_edit w_detached:1,40 trim
 echo "### G2 $b shadow d28m"; timeout 200 bin/memprof.$b data/d28m.snap load open0 shadow0 dropshared sub w_shadow:1,40 trim
 echo "### G3 $b deep+export d28m"; timeout 200 bin/memprof.$b data/d28m.snap load open0 dropshared deep export_upd export_snap trim
done
echo "### H1 peers: fresh peer stale writes, d1m"
timeout 280 bin/memprof.main data/d1m.snap load open0 dropshared sub detach_edit w_detached:1,100 stats w_detached:1,100 stats w_detached:1,200 stats w_detached:1,400 stats free_dc free_hc trim
echo "### H2 stable-ish: alt peers live writes, no export"
timeout 200 bin/memprof.main data/d1m.snap load open0 dropshared alt_writes:10000 stats alt_writes:40000 stats compact trim
echo "### H3 fresh peer plain writes"
timeout 200 bin/memprof.main data/d1m.snap load open0 dropshared fresh_peer_writes:1000 stats fresh_peer_writes:1000 stats compact trim
DHAT_OUT=out/dhat/h1.json timeout 280 bin/memprof.main.dhat data/d1m.snap load open0 dropshared sub detach_edit w_detached:1,400
DHAT_OUT=out/dhat/h2.json timeout 200 bin/memprof.main.dhat data/d1m.snap load open0 dropshared alt_writes:20000
echo "### I shallow/state-only reopen"
timeout 200 bin/memprof.main data/d28m.snap load open0 dropshared shallow_at:20000 open dropshared sub detach_edit w_detached:1,40 trim stats
timeout 200 bin/memprof.main data/d28m.snap load open0 dropshared state_only open len read_tail:20 append:1 deep trim stats
