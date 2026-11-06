#!/bin/bash
# D: cold-open operation matrix: each op in a fresh process, right after a zero-copy open.
cd /root/spikes/loro-mem
for D in d28m dlong; do
 for ops in "len" "read_tail:20" "read_at:20" "append:1" "push_turn:1" "sub append:1" "prep_upd,import_upd" "sub prep_upd,import_upd" "prep_upd,local_edit,import_upd" "sub prep_upd,local_edit,import_upd" "prep_upd,import_upd,export_recent" "f2v_head" "v2f_head" "export_snap" "export_upd" "deep" "read_all" "fork" "fork_at_head" "diff_back:1" "cursor" "undo_setup append:1" "detach_edit checkout_back:1" ; do
   args=$(echo "$ops" | tr ',' ' ')
   # keep the numeric comma forms out of this matrix (none used)
   echo "### $D | $ops"
   timeout 120 bin/memprof.main data/$D.snap load open0 dropshared $args 2>&1 | grep -vE "^stage|^load |^open0|^dropshared|^prep_upd|^    " 
 done
done
