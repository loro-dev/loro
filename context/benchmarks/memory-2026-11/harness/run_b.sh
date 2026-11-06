#!/bin/bash
# B: stage table on main; cold -> read -> write -> sub -> deep -> checkout -> free.
cd /root/spikes/loro-mem
for D in d28m dlong d1m; do
  echo "### $D zero-copy"
  timeout 200 bin/memprof.main data/$D.snap load open0 dropshared len read_tail:20 read_at:20 append:1 append:100 sub append:1 deep export_upd export_snap compact free_dc free_hc trim checkout_back:1 latest checkout_back:100 latest free_dc free_hc trim read_all deep_keep droppkeep trim stats
done
echo "### d28m copy"
timeout 200 bin/memprof.main data/d28m.snap load open len read_tail:20
echo "### d28m mmap"
timeout 200 bin/memprof.main data/d28m.snap openmmap len read_tail:20
