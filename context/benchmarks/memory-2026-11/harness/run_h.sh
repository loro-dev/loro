#!/bin/bash
cd /root/spikes/loro-mem
r() { n=$1; shift; f=$1; shift; DHAT_OUT=out/dhat/$n.json timeout 280 bin/memprof.proto.dhat data/$f.snap "$@" > out/dhat/$n.log 2>&1; echo "$n rc=$?"; }
r d28_import d28m load open0 dropshared prep_upd import_upd
r d28_stateonly d28m load open0 dropshared state_only open
r d28_deep d28m load open0 dropshared deep
r d28_exp2 d28m load open0 dropshared export_upd
r d28_forkat d28m load open0 dropshared fork_at_head
echo big-tracker
timeout 200 bin/memprof.main data/d28m.snap load open0 dropshared sub detach_edit checkout_back:1000000 free_dc latest free_dc free_hc trim
timeout 200 bin/memprof.main data/d28m.snap load open0 dropshared sub detach_edit checkout_back:6000000 free_dc latest free_dc free_hc trim
timeout 200 bin/memprof.main data/dlong.snap load open0 dropshared sub detach_edit checkout_back:40000 free_dc latest free_dc free_hc trim
