use loro::{ExportMode, Frontiers, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroText};
use rand::prelude::*;

fn pick_map(doc: &LoroDoc, rng: &mut StdRng, handles: &[LoroMap]) -> LoroMap {
    if handles.is_empty() || rng.gen_bool(0.2) {
        doc.get_map("root")
    } else {
        handles[rng.gen_range(0..handles.len())].clone()
    }
}

#[test]
fn random_fork_at_history() {
    for seed in 0..300u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let docs: Vec<LoroDoc> = (0..2)
            .map(|i| {
                let d = LoroDoc::new();
                d.set_peer_id(i + 1).unwrap();
                d
            })
            .collect();
        let mut maps: Vec<Vec<LoroMap>> = vec![vec![], vec![]];
        let mut lists: Vec<Vec<LoroMovableList>> = vec![vec![], vec![]];
        let mut texts: Vec<Vec<LoroText>> = vec![vec![], vec![]];
        let mut plain: Vec<Vec<LoroList>> = vec![vec![], vec![]];
        let mut versions = vec![];
        for _ in 0..40 {
            let i = rng.gen_range(0..2);
            let d = &docs[i];
            let key = format!("k{}", rng.gen_range(0..4));
            match rng.gen_range(0..9) {
                0 => {
                    let m = pick_map(d, &mut rng, &maps[i]);
                    if let Ok(c) = m.insert_container(&key, LoroMap::new()) {
                        maps[i].push(c);
                    }
                }
                1 => {
                    let m = pick_map(d, &mut rng, &maps[i]);
                    if let Ok(c) = m.insert_container(&key, LoroMovableList::new()) {
                        lists[i].push(c);
                    }
                }
                2 => {
                    let m = pick_map(d, &mut rng, &maps[i]);
                    if let Ok(c) = m.insert_container(&key, LoroText::new()) {
                        texts[i].push(c);
                    }
                }
                3 => {
                    let m = pick_map(d, &mut rng, &maps[i]);
                    let _ = m.delete(&key);
                }
                4 => {
                    if let Some(l) = lists[i].choose(&mut rng) {
                        let n = l.len();
                        let _ = l.insert(rng.gen_range(0..=n), rng.gen::<i32>());
                        if n > 1 {
                            let _ = l.mov(rng.gen_range(0..n), rng.gen_range(0..n));
                        }
                    }
                }
                5 => {
                    if let Some(l) = lists[i].choose(&mut rng) {
                        let n = l.len();
                        if n > 0 {
                            let _ = l.delete(rng.gen_range(0..n), 1);
                            let n = l.len();
                            if n > 0 {
                                let _ = l.set(rng.gen_range(0..n), 1);
                            }
                        }
                    }
                }
                6 => {
                    if let Some(t) = texts[i].choose(&mut rng) {
                        let n = t.len_unicode();
                        let _ = t.insert(rng.gen_range(0..=n), "ab");
                        if n > 2 {
                            let _ = t.delete(0, 1);
                            let _ = t.mark(0..1, "b", true);
                        }
                    }
                }
                7 => {
                    let m = pick_map(d, &mut rng, &maps[i]);
                    if let Ok(c) = m.insert_container(&key, LoroList::new()) {
                        let _ = c.push(1);
                        plain[i].push(c);
                    }
                }
                _ => {
                    let j = 1 - i;
                    docs[j]
                        .import(&docs[i].export(ExportMode::all_updates()).unwrap())
                        .unwrap();
                }
            }
            d.commit();
            versions.push(d.oplog_frontiers());
        }
        docs[0]
            .import(&docs[1].export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let doc = &docs[0];
        let reference = doc.fork();
        let head = doc.oplog_frontiers();
        let target = if rng.gen_bool(0.5) {
            head.clone()
        } else {
            versions[rng.gen_range(0..versions.len())].clone()
        };
        let tvv = doc.frontiers_to_vv(&target).unwrap();
        let fork = doc.fork_at(&target).unwrap();
        let nested = fork.fork_at(&target).unwrap();
        for f in [&fork, &nested] {
            let mut vs: Vec<Frontiers> = versions
                .iter()
                .filter(|v| {
                    let vv = doc.frontiers_to_vv(v).unwrap();
                    tvv.includes_vv(&vv)
                })
                .cloned()
                .collect();
            vs.push(Frontiers::default());
            vs.shuffle(&mut rng);
            for v in vs.iter().take(12) {
                reference.checkout(v).unwrap();
                f.checkout(v).unwrap();
                assert_eq!(
                    f.get_deep_value(),
                    reference.get_deep_value(),
                    "seed {seed}"
                );
            }
        }
    }
}
