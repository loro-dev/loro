#!/bin/bash
cd /root/spikes/loro-mem; mkdir -p out/dhat
r() { n=$1; shift; f=$1; shift; DHAT_OUT=out/dhat/$n.json timeout 280 bin/memprof.main.dhat data/$f.snap "$@" > out/dhat/$n.log 2>&1; echo "$n rc=$?"; }
r d28_cold d28m open
r d28_warm d28m load open0 dropshared len read_tail:20 append:1
r d28_export d28m load open0 dropshared len read_tail:20 append:1 export_upd
r d28_ckpt d28m load open0 dropshared len sub detach_edit checkout_back:1 latest
r d28_wdet d28m load open0 dropshared sub detach_edit w_detached:1,10
r d28_wshadow d28m load open0 shadow0 dropshared sub w_shadow:1,10
r dlong_warm dlong load open0 dropshared len read_tail:20 append:1
r dlong_export dlong load open0 dropshared len read_tail:20 append:1 export_upd
r dlong_ckpt dlong load open0 dropshared detach_edit checkout_back:1 latest
