import json,sys,re
d=json.load(open(sys.argv[1])); pat=re.compile(sys.argv[2]); top=int(sys.argv[3]); key=sys.argv[4] if len(sys.argv)>4 else 'eb'
ft=d['ftbl']
rows=[]
for pp in d['pps']:
    fr=[ft[i] for i in pp['fs']]
    if pp[key] and any(pat.search(f) for f in fr): rows.append((pp[key],pp.get(key+'k',0),fr))
rows.sort(key=lambda x:-x[0])
for b,k,fr in rows[:top]:
    print(f"--- {b/1048576:.2f}MiB blocks={k}")
    for f in fr:
        if re.search(r'loro|quick_cache|generic_btree|memprof',f):
            m=re.search(r': (.*) \((.*)\)$',f)
            s=(m.group(1) if m else f); s=re.sub(r'<[^<>]*>','',s)[:70]
            loc=(m.group(2).split('/')[-1] if m else '')
            print(f"    {s}  [{loc}]")
