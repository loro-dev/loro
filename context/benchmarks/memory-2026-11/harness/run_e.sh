#!/bin/bash
cd /root/spikes/loro-mem; mkdir -p out/dhat
F='^(stage|load|open0|dropshared|sub|detach_edit|    n=|peak|w_|checkout|latest|evict|free|trim|shadow0|import|diff|stats|prep)'
echo "### E1 import cold dhat"
DHAT_OUT=out/dhat/d28_import.json timeout 200 bin/memprof.main.dhat data/d28m.snap load open0 dropshared prep_upd import_upd > out/dhat/d28_import.log 2>&1
for v in main proto; do
 for D in d28m dlong; do
  e=""; [ $v = proto ] && e="evict"
  echo "### E2 $v $D cold checkout -1"
  timeout 120 bin/memprof.$v data/$D.snap load open0 dropshared sub detach_edit checkout_back:1 latest $e trim
  a=0; [ $v = proto ] && a=2
  echo "### E3 $v $D detached loop"
  timeout 200 bin/memprof.$v data/$D.snap load open0 dropshared sub detach_edit w_detached:1,40,$a w_detached:100,40,$a $e trim stats
  echo "### E4 $v $D shadow loop"
  timeout 200 bin/memprof.$v data/$D.snap load open0 shadow0 dropshared sub w_shadow:1,40,$a w_shadow:100,40,$a $e trim stats
 done
done
echo "### E5 proto export then evict"
timeout 120 bin/memprof.proto data/dlong.snap load open0 dropshared export_upd evict trim append:1 read_tail:20 stats
timeout 120 bin/memprof.proto data/d28m.snap load open0 dropshared export_upd evict trim append:1 read_tail:20 stats
