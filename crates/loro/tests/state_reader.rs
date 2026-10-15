//! `LoroDoc::read_state`: field-level reads must agree with `get_deep_value`.

use loro::{
    ContainerTrait, ContainerType, ExportMode, LoroCounter, LoroDoc, LoroList, LoroMap,
    LoroMovableList, LoroText, LoroTree, LoroValue, ReadEntry, StateReader,
};

/// Rebuild a deep value through the reader only.
fn deep<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> LoroValue {
    let c = match e {
        ReadEntry::Value(v) => {
            assert!(!matches!(v, LoroValue::Container(_)));
            return v;
        }
        ReadEntry::Container(c) => c,
    };
    match c.container_type() {
        ContainerType::Map => {
            let mut out = rustc_hash::FxHashMap::default();
            for k in r.map_keys(c) {
                let e = r.map_get(c, &k).unwrap();
                out.insert(k.to_string(), deep(r, e));
            }
            assert_eq!(out.len(), r.map_len(c));
            LoroValue::Map(out.into())
        }
        ContainerType::List | ContainerType::MovableList => {
            let entries = r.list_entries(c);
            assert_eq!(entries.len(), r.list_len(c));
            for (i, e) in entries.iter().enumerate() {
                // list_get agrees with list_entries.
                match (e, r.list_get(c, i).unwrap()) {
                    (ReadEntry::Value(a), ReadEntry::Value(b)) => assert_eq!(a, &b),
                    (ReadEntry::Container(a), ReadEntry::Container(b)) => assert_eq!(*a, b),
                    _ => panic!("list_get and list_entries disagree"),
                }
            }
            LoroValue::List(
                entries
                    .into_iter()
                    .map(|e| deep(r, e))
                    .collect::<Vec<_>>()
                    .into(),
            )
        }
        ContainerType::Text | ContainerType::Counter => r.value(c),
        _ => r.deep_value(c),
    }
}

fn assert_reader_matches(doc: &LoroDoc, root: &LoroMap) {
    let expected = root.get_deep_value();
    let got = doc.read_state(|r| {
        let c = r.container(root).unwrap();
        deep(r, ReadEntry::Container(c))
    });
    assert_eq!(got, expected);
}

fn sample(doc: &LoroDoc) -> LoroMap {
    let root = doc.get_map("root");
    root.insert("n", 1).unwrap();
    root.insert("s", "str").unwrap();
    root.insert("null", LoroValue::Null).unwrap();
    let list = root.insert_container("list", LoroList::new()).unwrap();
    list.push(1).unwrap();
    let inner = list.push_container(LoroMap::new()).unwrap();
    inner.insert("k", "v").unwrap();
    let ml = root.insert_container("ml", LoroMovableList::new()).unwrap();
    ml.push("a").unwrap();
    ml.push_container(LoroText::new())
        .unwrap()
        .insert(0, "in list")
        .unwrap();
    let text = root.ensure_mergeable_text("text").unwrap();
    text.insert(0, "hello").unwrap();
    let counter = root.ensure_mergeable_counter("count").unwrap();
    counter.increment(2.5).unwrap();
    let nested = root.ensure_mergeable_map("nested").unwrap();
    let deeper = nested.ensure_mergeable_map("deeper").unwrap();
    deeper.insert("x", true).unwrap();
    let regular = root
        .insert_container("regular", LoroCounter::new())
        .unwrap();
    regular.increment(1.0).unwrap();
    let tree = root.insert_container("tree", LoroTree::new()).unwrap();
    let node = tree.create(None).unwrap();
    tree.get_meta(node).unwrap().insert("m", 1).unwrap();
    root
}

#[test]
fn reader_matches_deep_value() {
    let doc = LoroDoc::new();
    let root = sample(&doc);
    // Uncommitted changes are visible, like handler reads.
    assert_reader_matches(&doc, &root);
    doc.commit();
    assert_reader_matches(&doc, &root);

    // A snapshot-loaded document reads lazily decoded containers.
    let loaded = LoroDoc::new();
    loaded
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_reader_matches(&loaded, &loaded.get_map("root"));
}

#[test]
fn reader_resolves_mergeable_markers_like_map_get() {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    root.ensure_mergeable_map("child").unwrap();
    // A marker copied to another key does not activate a child there.
    let marker = loro_common::mergeable_marker(&root.id(), "child", ContainerType::Map);
    root.insert("copy", marker).unwrap();
    doc.read_state(|r| {
        let m = r.container(&root).unwrap();
        assert!(matches!(
            r.map_get(m, "child"),
            Some(ReadEntry::Container(_))
        ));
        assert!(matches!(
            r.map_get(m, "copy"),
            Some(ReadEntry::Value(LoroValue::Binary(_)))
        ));
        assert!(r.map_get(m, "missing").is_none());
    });
    assert_reader_matches(&doc, &root);

    // Deleting the key hides the child.
    root.delete("child").unwrap();
    doc.read_state(|r| {
        let m = r.container(&root).unwrap();
        assert!(r.map_get(m, "child").is_none());
    });
}

#[test]
fn container_refs_are_per_document() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    let ma = a.get_map("m");
    let mb = b.get_map("m");
    a.read_state(|r| {
        assert!(r.container(&ma).is_some());
        assert!(r.container(&mb).is_none());
        assert!(r.container(&LoroMap::new()).is_none());
    });
}

#[test]
fn deleted_containers_are_reported() {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let child = root.insert_container("c", LoroMap::new()).unwrap();
    root.delete("c").unwrap();
    doc.read_state(|r| {
        let c = r.container(&child).unwrap();
        assert!(r.is_deleted(c));
    });
}

/// Random documents built from concurrent peers, compared at every sync.
#[test]
fn reader_matches_deep_value_on_random_docs() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n.max(1) as u64) as usize
        }
    }
    for seed in 0..60u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let docs: Vec<LoroDoc> = (0..2)
            .map(|i| {
                let d = LoroDoc::new();
                d.set_peer_id(i + 1).unwrap();
                d
            })
            .collect();
        for _ in 0..60 {
            let d = &docs[rng.below(2)];
            let root = d.get_map("root");
            let key = format!("k{}", rng.below(4));
            match rng.below(7) {
                0 => root.insert(&key, rng.below(100) as i64).unwrap(),
                1 => {
                    let m = root.ensure_mergeable_map(&key).unwrap_or_else(|_| {
                        root.delete(&key).unwrap();
                        root.ensure_mergeable_map(&key).unwrap()
                    });
                    m.insert("v", rng.below(10) as i64).unwrap();
                }
                2 => {
                    let l = root.ensure_mergeable_list(&key).unwrap_or_else(|_| {
                        root.delete(&key).unwrap();
                        root.ensure_mergeable_list(&key).unwrap()
                    });
                    l.push(rng.below(10) as i64).unwrap();
                }
                3 => {
                    root.insert_container(&key, LoroMap::new())
                        .unwrap()
                        .insert("r", 1)
                        .unwrap();
                }
                4 => {
                    let _ = root.delete(&key);
                }
                5 => {
                    let t = root.ensure_mergeable_text(&key).unwrap_or_else(|_| {
                        root.delete(&key).unwrap();
                        root.ensure_mergeable_text(&key).unwrap()
                    });
                    t.insert(0, "x").unwrap();
                }
                _ => {
                    let (a, b) = (&docs[0], &docs[1]);
                    a.import(&b.export(ExportMode::all_updates()).unwrap())
                        .unwrap();
                    b.import(&a.export(ExportMode::all_updates()).unwrap())
                        .unwrap();
                }
            }
            assert_reader_matches(d, &root);
        }
    }
}

/// `MovableListState::iter` walks the list once; it must yield what indexed reads yield,
/// after moves, sets and deletes.
#[test]
fn movable_list_iteration_matches_indexed_reads() {
    let doc = LoroDoc::new();
    let ml = doc.get_movable_list("ml");
    for i in 0..50 {
        ml.push(i).unwrap();
    }
    for i in 0..40usize {
        let len = ml.len();
        ml.mov(i % len, (i * 7) % len).unwrap();
        if i % 5 == 0 {
            ml.set(i % len, -(i as i64)).unwrap();
        }
        if i % 9 == 0 {
            ml.delete(i % len, 1).unwrap();
        }
    }
    let other = LoroDoc::new();
    other
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    other.get_movable_list("ml").mov(0, 3).unwrap();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let by_index: Vec<LoroValue> = (0..ml.len())
        .map(|i| ml.get(i).unwrap().into_value().unwrap())
        .collect();
    assert_eq!(ml.get_value(), LoroValue::List(by_index.clone().into()));
    let read = doc.read_state(|r| {
        let c = r.container(&ml).unwrap();
        r.list_entries(c)
            .into_iter()
            .map(|e| match e {
                ReadEntry::Value(v) => v,
                ReadEntry::Container(_) => unreachable!(),
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(read, by_index);
}
