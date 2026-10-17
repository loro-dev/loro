use loro::{ExportMode, LoroDoc, LoroMap, UndoManager};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn run(seed: u64) {
    let docs: Vec<_> = (1..=3)
        .map(|peer| {
            let doc = LoroDoc::new();
            doc.set_peer_id(peer).unwrap();
            doc
        })
        .collect();
    let first = docs[0].get_movable_list("list");
    for i in 0..4 {
        first.push(i).unwrap();
    }
    first
        .push_container(LoroMap::new())
        .unwrap()
        .insert("value", 7)
        .unwrap();
    docs[0].commit();
    let initial = docs[0].export(ExportMode::all_updates()).unwrap();
    docs[1].import(&initial).unwrap();
    docs[2].import(&initial).unwrap();
    let mut managers: Vec<_> = docs
        .iter()
        .map(|doc| {
            let mut undo = UndoManager::new(doc);
            undo.set_merge_interval(0);
            undo
        })
        .collect();
    let mut random = StdRng::seed_from_u64(seed);
    for step in 0..120 {
        let peer = random.gen_range(0..3);
        let doc = &docs[peer];
        let list = doc.get_movable_list("list");
        let n = list.len();
        let action = if n > 24 { 2 } else { random.gen_range(0..10) };
        match action {
            0 => list.insert(random.gen_range(0..=n), step).unwrap(),
            1 => {
                list.insert_container(random.gen_range(0..=n), LoroMap::new())
                    .unwrap()
                    .insert("value", step)
                    .unwrap();
            }
            2 if n > 0 => list.delete(random.gen_range(0..n), 1).unwrap(),
            3 if n > 0 => list
                .mov(random.gen_range(0..n), random.gen_range(0..n))
                .unwrap(),
            4 if n > 0 => list.set(random.gen_range(0..n), step).unwrap(),
            5 if n > 0 => {
                list.set_container(random.gen_range(0..n), LoroMap::new())
                    .unwrap()
                    .insert("replacement", step)
                    .unwrap();
            }
            6 if n > 0 => {
                if let Some(map) = list
                    .get(random.gen_range(0..n))
                    .and_then(|v| v.into_container().ok())
                    .and_then(|c| c.into_map().ok())
                {
                    map.insert("edit", step).unwrap();
                }
            }
            7 => {
                let target = (peer + random.gen_range(1..3)) % 3;
                let bytes = doc.export(ExportMode::all_updates()).unwrap();
                docs[target].import(&bytes).unwrap();
            }
            8 => {
                managers[peer]
                    .undo()
                    .unwrap_or_else(|e| panic!("seed={seed} step={step} undo: {e:?}"));
            }
            9 => {
                managers[peer]
                    .redo()
                    .unwrap_or_else(|e| panic!("seed={seed} step={step} redo: {e:?}"));
            }
            _ => {}
        }
        doc.commit();
    }
    for source in &docs[1..] {
        docs[0]
            .import(&source.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    for target in &docs[1..] {
        target
            .import(&docs[0].export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(
            target.get_deep_value(),
            docs[0].get_deep_value(),
            "seed={seed}"
        );
    }
    for mode in [ExportMode::Snapshot, ExportMode::all_updates()] {
        let restored = LoroDoc::new();
        restored.import(&docs[0].export(mode).unwrap()).unwrap();
        assert_eq!(
            restored.get_deep_value(),
            docs[0].get_deep_value(),
            "seed={seed}"
        );
    }
}

#[test]
fn three_peer_sync_move_set_undo_redo_with_scalar_and_map_values() {
    if let Ok(count) = std::env::var("LORO_UNDO_RANDOM_SEEDS") {
        for seed in 0..count.parse::<u64>().unwrap() {
            run(seed);
        }
    } else {
        for seed in [0, 1, 2, 3, 17, 203, 1340] {
            run(seed);
        }
    }
}
