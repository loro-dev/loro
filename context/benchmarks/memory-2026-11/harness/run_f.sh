#!/bin/bash
cd /root/spikes/loro-mem
for spec in "new 2000" "newsub 2000" "newwrite 2000" "open 2000 tiny" "open 2000 s10" "open 1000 s100" "open 200 s1000" "open0 2000 tiny" "open0 2000 s10" "open0 1000 s100" "open0 200 s1000" "open0read 2000 s10" "open0write 2000 s10" "openwrite 2000 s10" "open0 100 data/d1m.snap" "open0 20 data/d28m.snap" "open0write 20 data/d28m.snap"; do
  timeout 120 bin/multidoc.main $spec
done
for n in 100 1000 5000; do timeout 120 bin/multidoc.main open0 $n s10; done
for b in je mi; do for spec in "open0 2000 s10" "open0write 2000 s10"; do echo "[$b]"; timeout 120 bin/multidoc.main.$b $spec; done; done
timeout 120 bin/multidoc.v117 open 2000 s10; timeout 120 bin/multidoc.v117 new 2000
DHAT_OUT=out/dhat/m_new.json timeout 100 bin/multidoc.main.dhat new 200
DHAT_OUT=out/dhat/m_open.json timeout 100 bin/multidoc.main.dhat open0 200 s10
DHAT_OUT=out/dhat/m_write.json timeout 100 bin/multidoc.main.dhat open0write 200 s10
