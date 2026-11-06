#!/bin/bash
# A: baseline 1.17.1 vs main, one process per mechanism, same stage script as q2 (d=1,10,100).
cd /root/spikes/loro-mem
D=data/d28m.snap
for v in v117 main; do
  for m in detached shadow fork; do
    S="load open read_tail:1"
    [ $m = shadow ] && S="$S shadow"
    S="$S sub"
    [ $m = detached ] && S="$S detach_edit"
    [ $m = shadow ] && S="$S"
    n=40; [ $m = fork ] && n=15
    echo "### $v $m"
    timeout 280 bin/memprof.$v $D $S w_$m:1,$n w_$m:100,$n stats
  done
done
