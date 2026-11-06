#!/usr/bin/env python3
"""Attribute dhat live-at-end bytes (eb) to Loro components by stack frames."""
import json, re, sys, collections
RULES = [
 ("arena.containers/parents", r"SharedArena::(register_container|set_parent|_set_parent|register)|arena::.*(register|set_parent|container_idx|id_to_idx)|register_container_and_parent_link"),
 ("arena.strings", r"StrArena|str_arena|alloc_str"),
 ("kv.block_cache(decompressed)", r"read_block_cached|Block::decode|Block::from_bytes|new_block_cache|quick_cache"),
 ("oplog.parsed_blocks", r"ChangesBlockBytes|block_encode::decode_block|ensure_changes|decode_block"),
 ("oplog.change_store(live)", r"insert_change|ChangesBlock::|ChangeStore::insert"),
 ("oplog.dag", r"loro_dag|AppDag|ensure_vv_for|lazy_load"),
 ("history_cache", r"history_cache|ContainerHistoryCache|MapHistoryCache"),
 ("diff_calc", r"diff_calc|DiffCalculator|tracker|Tracker"),
 ("state.richtext", r"richtext_state|RichtextState"),
 ("state.list", r"list_state|ListState"),
 ("state.map", r"map_state|MapState"),
 ("state.lazy_wrapper", r"container_wrapper|ContainerWrapper|LazyContainerData"),
 ("state.store", r"inner_store|InnerStore|container_store|ContainerStore|kv_wrapper|KvWrapper"),
 ("state.other", r"loro_internal::state"),
 ("kv.sstable", r"loro_kv_store|SsTable|MemKvStore|mem_store"),
 ("oplog.other", r"loro_internal::oplog"),
 ("doc.new", r"LoroDoc::new|new_inner"),
 ("handler/harness", r"loro_mem|memprof|multidoc"),
]
RULES=[(n,re.compile(p)) for n,p in RULES]
d=json.load(open(sys.argv[1]))
key=sys.argv[2] if len(sys.argv)>2 and sys.argv[2] in("eb","gb","tb") else "eb"
show=int(sys.argv[3]) if len(sys.argv)>3 else 0
ft=d["ftbl"]
agg=collections.Counter(); blocks=collections.Counter(); ex=collections.defaultdict(list)
tot=0
for pp in d["pps"]:
    b=pp[key]
    if not b: continue
    tot+=b
    fr=[ft[i] for i in pp["fs"]]
    cat=None
    for n,r in RULES:
        if any(r.search(f) for f in fr): cat=n;break
    cat=cat or "other"
    agg[cat]+=b; blocks[cat]+=pp.get(key+"k",0)
    ex[cat].append((b,fr))
print(f"total {key}={tot/1048576:.2f} MiB")
for c,b in agg.most_common():
    print(f"  {c:<34}{b/1048576:>9.2f} MiB {100*b/tot:>5.1f}%  blocks={blocks[c]}")
if show:
    for c,b in agg.most_common():
        print("==",c)
        for b2,fr in sorted(ex[c],key=lambda x:-x[0])[:show]:
            print(f"   {b2/1048576:.2f}MiB")
            keep=[f.split(': ',1)[-1][:110] for f in fr if ('loro' in f or 'quick_cache' in f or 'generic_btree' in f)][:7]
            for f in keep: print("      ",f)
