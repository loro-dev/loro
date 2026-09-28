//! Twin oracle for containers of a type unknown to this version (written by
//! a newer Loro): a forged doc holding them and a twin holding Counters in
//! their place get the same random edits. Every `apply_diff` (full state and
//! incremental), `revert_to` and undo/redo on the forged doc must either be
//! rejected without changing it, or succeed with the same result as on the
//! twin. The atomicity check alone can't see an `Ok` with a wrong state.
//!
//! `LORO_UNKNOWN_TWIN_SEEDS` / `LORO_UNKNOWN_TWIN_START` run more seeds.

mod unknown_container_support;

use loro::{
    LoroDoc, LoroList, LoroMap, LoroMovableList, LoroResult, LoroTree, TreeID, UndoManager,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use unknown_container_support::*;

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Tree node ids and counter values differ between the twins
fn norm(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Number(n) if n.is_f64() => Value::Null,
        Value::Array(a) => Value::Array(a.into_iter().map(norm).collect()),
        Value::Object(o) => {
            let tree_node = o.contains_key("fractional_index") && o.contains_key("meta");
            Value::Object(
                o.into_iter()
                    .filter(|(k, _)| {
                        !(tree_node && (k == "id" || k == "parent" || k == "fractional_index"))
                    })
                    .map(|(k, v)| (k, norm(v)))
                    .collect(),
            )
        }
        v => v,
    }
}

fn nstate(doc: &LoroDoc) -> serde_json::Value {
    norm(state(doc).0)
}

fn fill_nm(nm: &LoroMap, with_counters: bool) {
    let k = nm.ensure_mergeable_list("k").unwrap();
    if with_counters {
        edited_counter(k.insert_container(0, counter()).unwrap());
    }
    k.push(1).unwrap();
    let s = nm.ensure_mergeable_map("s").unwrap();
    if with_counters {
        edited_counter(s.insert_container("u", counter()).unwrap());
    }
    s.insert("x", 1).unwrap();
    let mv = nm.ensure_mergeable_movable_list("mv").unwrap();
    mv.push("a").unwrap();
    if with_counters {
        edited_counter(mv.insert_container(1, counter()).unwrap());
    }
    mv.push("b").unwrap();
    nm.insert("plain", 1).unwrap();
}

fn build(doc: &LoroDoc) {
    let m = doc.get_map("m");
    let l = m.insert_container("l", LoroList::new()).unwrap();
    edited_counter(l.insert_container(0, counter()).unwrap());
    l.push(1).unwrap();
    l.insert_container(2, counter()).unwrap();
    let ml = m.insert_container("ml", LoroMovableList::new()).unwrap();
    ml.push("a").unwrap();
    edited_counter(ml.insert_container(1, counter()).unwrap());
    ml.push("b").unwrap();
    ml.insert_container(3, counter()).unwrap();
    let mm = m.insert_container("mm", LoroMap::new()).unwrap();
    edited_counter(mm.insert_container("u", counter()).unwrap());
    mm.insert("x", 1).unwrap();
    let merge = m.ensure_mergeable_list("merge").unwrap();
    merge.push(2).unwrap();
    edited_counter(merge.insert_container(0, counter()).unwrap());
    let smap = m.ensure_mergeable_map("smap").unwrap();
    edited_counter(smap.insert_container("u", counter()).unwrap());
    smap.insert("x", 1).unwrap();
    let sl = smap.insert_container("l", LoroList::new()).unwrap();
    edited_counter(sl.insert_container(0, counter()).unwrap());
    sl.push(1).unwrap();
    // normal map holding mergeable children that hold U
    let nm = m.insert_container("nm", LoroMap::new()).unwrap();
    fill_nm(&nm, true);
    // normal -> mergeable -> normal -> mergeable
    let deep = m.insert_container("deep", LoroMap::new()).unwrap();
    let s2 = deep.ensure_mergeable_map("s2").unwrap();
    let n = s2.insert_container("n", LoroMap::new()).unwrap();
    let k = n.ensure_mergeable_list("k").unwrap();
    edited_counter(k.insert_container(0, counter()).unwrap());
    k.push(3).unwrap();
    // list of maps with mergeable children
    let l2 = doc.get_list("l2");
    let lm = l2.insert_container(0, LoroMap::new()).unwrap();
    fill_nm(&lm, true);
    l2.insert_container(1, counter()).unwrap();
    // tree whose metas hold U and mergeable lists holding U
    let tree = doc.get_tree("tree");
    let n1 = tree.create(None).unwrap();
    let n2 = tree.create(n1).unwrap();
    let n3 = tree.create(None).unwrap();
    for n in [n1, n2, n3] {
        let meta = tree.get_meta(n).unwrap();
        meta.insert_container("u", counter()).unwrap();
        meta.insert("n", 1).unwrap();
        let mk = meta.ensure_mergeable_list("k").unwrap();
        edited_counter(mk.insert_container(0, counter()).unwrap());
        mk.push(1).unwrap();
    }
    // mergeable tree whose node meta holds U and a mergeable movable list with U
    let st = m.ensure_mergeable_tree("st").unwrap();
    let sn = st.create(None).unwrap();
    let sm = st.get_meta(sn).unwrap();
    edited_counter(sm.insert_container("u", counter()).unwrap());
    let smk = sm.ensure_mergeable_movable_list("mk").unwrap();
    smk.push("q").unwrap();
    edited_counter(smk.insert_container(1, counter()).unwrap());
    // movable list of maps with mergeable children
    let mlm = doc.get_movable_list("mlm");
    let e = mlm.insert_container(0, LoroMap::new()).unwrap();
    fill_nm(&e, true);
    mlm.push("z").unwrap();
    let e2 = mlm.insert_container(2, LoroMap::new()).unwrap();
    e2.insert_container("u", counter()).unwrap();
}

fn pick<T: Clone>(rng: &mut StdRng, v: &[T]) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        Some(v[rng.gen_range(0..v.len())].clone())
    }
}

fn alive_nodes(tree: &LoroTree) -> Vec<TreeID> {
    tree.nodes()
        .into_iter()
        .filter(|n| !tree.is_node_deleted(n).unwrap())
        .collect()
}

fn edit_nm(nm: &LoroMap, rng: &mut StdRng) {
    match rng.gen_range(0..7) {
        0 => {
            nm.delete("k").unwrap();
        }
        1 => {
            let k = nm.ensure_mergeable_list("k").unwrap();
            if !k.is_empty() && rng.gen_bool(0.5) {
                k.delete(rng.gen_range(0..k.len()), 1).unwrap();
            } else {
                k.push(rng.gen_range(0..10)).unwrap();
            }
        }
        2 => {
            nm.delete("s").unwrap();
        }
        3 => {
            let s = nm.ensure_mergeable_map("s").unwrap();
            if rng.gen_bool(0.5) {
                s.delete("u").unwrap();
            } else {
                s.insert("x", rng.gen_range(0..10)).unwrap();
            }
        }
        4 => {
            nm.delete("mv").unwrap();
        }
        _ => {
            let mv = nm.ensure_mergeable_movable_list("mv").unwrap();
            let len = mv.len();
            match (len, rng.gen_range(0..3)) {
                (0, _) => mv.push("n").unwrap(),
                (_, 0) => mv.delete(rng.gen_range(0..len), 1).unwrap(),
                (_, 1) => mv.set(rng.gen_range(0..len), rng.gen_range(0..10)).unwrap(),
                _ => mv
                    .mov(rng.gen_range(0..len), rng.gen_range(0..len))
                    .unwrap(),
            }
        }
    }
}

fn random_edit(doc: &LoroDoc, rng: &mut StdRng) {
    let m = doc.get_map("m");
    let child = |key: &str| m.get(key).and_then(|v| v.into_container().ok());
    match rng.gen_range(0..17) {
        0 => {
            let lists = ["l"]
                .into_iter()
                .filter_map(|k| child(k).and_then(|c| c.into_list().ok()))
                .collect::<Vec<_>>();
            if let Some(l) = pick(rng, &lists) {
                if !l.is_empty() {
                    l.delete(rng.gen_range(0..l.len()), 1).unwrap();
                } else {
                    l.push(rng.gen_range(0..10)).unwrap();
                }
            }
        }
        1 | 2 => {
            if let Some(ml) = child("ml").and_then(|c| c.into_movable_list().ok()) {
                let len = ml.len();
                match (len, rng.gen_range(0..3)) {
                    (0, _) => ml.push("z").unwrap(),
                    (_, 0) => ml.delete(rng.gen_range(0..len), 1).unwrap(),
                    (_, 1) => ml.set(rng.gen_range(0..len), rng.gen_range(0..10)).unwrap(),
                    _ => ml
                        .mov(rng.gen_range(0..len), rng.gen_range(0..len))
                        .unwrap(),
                }
            }
        }
        3 => {
            let keys = ["l", "ml", "mm", "nm", "deep"];
            m.delete(keys[rng.gen_range(0..keys.len())]).unwrap();
        }
        4 => {
            // recreate a key with a new (known) container, possibly several times
            match rng.gen_range(0..3) {
                0 => {
                    let nm = m.insert_container("nm", LoroMap::new()).unwrap();
                    fill_nm(&nm, false);
                }
                1 => {
                    let l = m.insert_container("l", LoroList::new()).unwrap();
                    l.push(5).unwrap();
                }
                _ => {
                    let deep = m.insert_container("deep", LoroMap::new()).unwrap();
                    deep.ensure_mergeable_map("s2")
                        .unwrap()
                        .insert("y", 1)
                        .unwrap();
                }
            }
        }
        5 => {
            if let Some(nm) = child("nm").and_then(|c| c.into_map().ok()) {
                edit_nm(&nm, rng);
            }
        }
        6 => {
            let tree = doc.get_tree("tree");
            let alive = alive_nodes(&tree);
            match rng.gen_range(0..4) {
                0 if !alive.is_empty() => tree.delete(pick(rng, &alive).unwrap()).unwrap(),
                1 if alive.len() >= 2 => {
                    let a = pick(rng, &alive).unwrap();
                    let b = pick(rng, &alive).unwrap();
                    let _ = tree.mov(a, b);
                }
                2 if !alive.is_empty() => {
                    let n = pick(rng, &alive).unwrap();
                    let meta = tree.get_meta(n).unwrap();
                    edit_nm(&meta, rng);
                }
                _ => {
                    let n = tree.create(None).unwrap();
                    let meta = tree.get_meta(n).unwrap();
                    meta.insert("n", 2).unwrap();
                    meta.ensure_mergeable_list("k").unwrap().push(7).unwrap();
                }
            }
        }
        7 => {
            let merge = m.ensure_mergeable_list("merge").unwrap();
            if !merge.is_empty() && rng.gen_bool(0.6) {
                merge.delete(rng.gen_range(0..merge.len()), 1).unwrap();
            } else if rng.gen_bool(0.5) {
                m.delete("merge").unwrap();
            } else {
                merge.push(rng.gen_range(0..10)).unwrap();
            }
        }
        8 => {
            let smap = m.ensure_mergeable_map("smap").unwrap();
            match rng.gen_range(0..4) {
                0 => m.delete("smap").unwrap(),
                1 => smap.delete("u").unwrap(),
                2 => {
                    if let Some(Ok(sl)) = smap
                        .get("l")
                        .and_then(|v| v.into_container().ok())
                        .map(|c| c.into_list())
                    {
                        if !sl.is_empty() {
                            sl.delete(0, 1).unwrap();
                        }
                    }
                }
                _ => smap.insert("x", rng.gen_range(0..10)).unwrap(),
            }
        }
        9 => {
            let st = m.ensure_mergeable_tree("st").unwrap();
            let alive = alive_nodes(&st);
            match rng.gen_range(0..4) {
                0 => m.delete("st").unwrap(),
                1 if !alive.is_empty() => st.delete(pick(rng, &alive).unwrap()).unwrap(),
                2 if !alive.is_empty() => {
                    let n = pick(rng, &alive).unwrap();
                    let meta = st.get_meta(n).unwrap();
                    match rng.gen_range(0..3) {
                        0 => meta.delete("mk").unwrap(),
                        1 => meta.delete("u").unwrap(),
                        _ => {
                            let mk = meta.ensure_mergeable_movable_list("mk").unwrap();
                            if mk.len() >= 2 {
                                mk.mov(0, mk.len() - 1).unwrap();
                            } else {
                                mk.push("w").unwrap();
                            }
                        }
                    }
                }
                _ => {
                    st.create(None).unwrap();
                }
            }
        }
        10 => {
            if let Some(deep) = child("deep").and_then(|c| c.into_map().ok()) {
                match rng.gen_range(0..4) {
                    0 => deep.delete("s2").unwrap(),
                    1 => {
                        let s2 = deep.ensure_mergeable_map("s2").unwrap();
                        s2.delete("n").unwrap();
                    }
                    2 => {
                        let s2 = deep.ensure_mergeable_map("s2").unwrap();
                        if let Some(Ok(n)) = s2
                            .get("n")
                            .and_then(|v| v.into_container().ok())
                            .map(|c| c.into_map())
                        {
                            if rng.gen_bool(0.5) {
                                n.delete("k").unwrap();
                            } else {
                                n.ensure_mergeable_list("k").unwrap().push(9).unwrap();
                            }
                        }
                    }
                    _ => {
                        deep.ensure_mergeable_map("s2")
                            .unwrap()
                            .insert("z", 1)
                            .unwrap();
                    }
                }
            }
        }
        11 => {
            let l2 = doc.get_list("l2");
            match rng.gen_range(0..3) {
                0 if !l2.is_empty() => l2.delete(rng.gen_range(0..l2.len()), 1).unwrap(),
                1 if !l2.is_empty() => {
                    if let Some(Ok(lm)) = l2
                        .get(0)
                        .and_then(|v| v.into_container().ok())
                        .map(|c| c.into_map())
                    {
                        edit_nm(&lm, rng);
                    }
                }
                _ => {
                    let lm = l2.insert_container(0, LoroMap::new()).unwrap();
                    fill_nm(&lm, false);
                }
            }
        }
        12 | 13 => {
            let mlm = doc.get_movable_list("mlm");
            let len = mlm.len();
            match (len, rng.gen_range(0..5)) {
                (0, _) => {
                    let e = mlm.push_container(LoroMap::new()).unwrap();
                    fill_nm(&e, false);
                }
                (_, 0) => mlm.delete(rng.gen_range(0..len), 1).unwrap(),
                (_, 1) => mlm
                    .mov(rng.gen_range(0..len), rng.gen_range(0..len))
                    .unwrap(),
                (_, 2) => {
                    if let Some(Ok(e)) = mlm
                        .get(rng.gen_range(0..len))
                        .and_then(|v| v.into_container().ok())
                        .map(|c| c.into_map())
                    {
                        edit_nm(&e, rng);
                    }
                }
                (_, 3) => mlm
                    .set(rng.gen_range(0..len), rng.gen_range(0..10))
                    .unwrap(),
                _ => {
                    let e = mlm
                        .insert_container(rng.gen_range(0..=len), LoroMap::new())
                        .unwrap();
                    fill_nm(&e, false);
                }
            }
        }
        _ => {
            let t = doc.get_text("t");
            let pos = rng.gen_range(0..=t.len_unicode());
            t.insert(pos, "x").unwrap();
        }
    }
}

enum Outcome {
    Ok,
    Rejected,
    OtherErr,
}

/// Runs `f`, which must not change `doc` when it fails.
fn run<T>(label: &str, doc: &LoroDoc, f: impl FnOnce(&LoroDoc) -> LoroResult<T>) -> Outcome {
    let before = state(doc);
    match f(doc) {
        Ok(_) => Outcome::Ok,
        Err(e) => {
            assert_eq!(
                state(doc),
                before,
                "{label}: partially applied before {e:?}"
            );
            if is_unknown_err(&e) {
                Outcome::Rejected
            } else {
                Outcome::OtherErr
            }
        }
    }
}

#[derive(Default)]
struct Counts {
    ok: usize,
    rejected: usize,
}

/// Compares the forged doc `u` with its twin `k` after an operation that
/// succeeded on both. Returns whether `u` was rejected.
fn compare(
    label: &str,
    counts: &mut Counts,
    u: &LoroDoc,
    ru: Outcome,
    k: &LoroDoc,
    rk: Outcome,
) -> bool {
    match ru {
        Outcome::Ok => {
            counts.ok += 1;
            if matches!(rk, Outcome::Ok) {
                assert_eq!(
                    nstate(u),
                    nstate(k),
                    "{label}: Ok but differs from the twin"
                );
            }
            false
        }
        Outcome::Rejected => {
            counts.rejected += 1;
            true
        }
        Outcome::OtherErr => false,
    }
}

fn case(seed: u64, counts: &mut Counts) {
    let u = forge_as(build, true);
    let k = forge_as(build, false);
    assert_eq!(nstate(&u), nstate(&k));
    let mut uu = UndoManager::new(&u);
    let mut ku = UndoManager::new(&k);
    let mut rng_u = StdRng::seed_from_u64(seed);
    let mut rng_k = StdRng::seed_from_u64(seed);
    let mut versions = vec![u.state_frontiers()];
    let n = rng_u.gen_range(3..14);
    rng_k.gen_range(3..14);
    for _ in 0..n {
        random_edit(&u, &mut rng_u);
        random_edit(&k, &mut rng_k);
        let commit = rng_u.gen_bool(0.6);
        rng_k.gen_bool(0.6);
        if commit {
            u.commit();
            k.commit();
            versions.push(u.state_frontiers());
        }
    }
    u.commit();
    k.commit();
    versions.push(u.state_frontiers());
    assert_eq!(nstate(&u), nstate(&k), "seed {seed}: twins diverged");

    let latest = u.state_frontiers();
    for (vi, target) in versions.iter().enumerate() {
        for full_state in [true, false] {
            let label = format!("seed {seed} v{vi} apply_diff full_state={full_state}");
            let (du, dk) = (u.fork(), k.fork());
            let apply = |d: &LoroDoc| {
                let mut batch = d.diff(&latest, target)?;
                batch.set_full_state(full_state);
                d.apply_diff(batch)
            };
            let ru = run(&label, &du, apply);
            let rk = run(&label, &dk, apply);
            compare(&label, counts, &du, ru, &dk, rk);
        }
        let label = format!("seed {seed} v{vi} revert_to");
        let (du, dk) = (u.fork(), k.fork());
        let ru = run(&label, &du, |d| d.revert_to(target));
        let rk = run(&label, &dk, |d| d.revert_to(target));
        compare(&label, counts, &du, ru, &dk, rk);
    }

    // After a rejected step the stacks differ, so compare until then
    let mut same = true;
    for i in 0..versions.len() + 1 {
        let label = format!("seed {seed} undo#{i}");
        let ru = run(&label, &u, |_| uu.undo());
        let rk = run(&label, &k, |_| ku.undo());
        if same {
            same = !compare(&label, counts, &u, ru, &k, rk);
        }
    }
    for i in 0..versions.len() + 1 {
        let label = format!("seed {seed} redo#{i}");
        let ru = run(&label, &u, |_| uu.redo());
        let rk = run(&label, &k, |_| ku.redo());
        if same {
            same = !compare(&label, counts, &u, ru, &k, rk);
        }
    }
}

#[test]
fn unknown_containers_match_their_known_twin() {
    let start = env_u64("LORO_UNKNOWN_TWIN_START", 0);
    let seeds = env_u64("LORO_UNKNOWN_TWIN_SEEDS", 60);
    let mut counts = Counts::default();
    for seed in start..start + seeds {
        case(seed, &mut counts);
    }
    println!("ok {} rejected {}", counts.ok, counts.rejected);
    // Both outcomes must be exercised
    assert!(counts.ok > 4 * seeds as usize, "ok {}", counts.ok);
    assert!(
        counts.rejected > 2 * seeds as usize,
        "rejected {}",
        counts.rejected
    );
}

/// Steps insert unique chars into a text; some also delete an unknown
/// container, which can't be undone. After undoing everything, the text holds
/// exactly the chars of the rejected steps, and redoing restores it all.
#[test]
fn undo_keeps_exactly_the_rejected_steps() {
    let seeds = env_u64("LORO_UNKNOWN_TWIN_SEEDS", 60) * 5;
    for seed in 0..seeds {
        let doc = forge(|doc| {
            let us = doc.get_list("us");
            for _ in 0..6 {
                edited_counter(us.push_container(counter()).unwrap());
            }
        });
        let mut undo = UndoManager::new(&doc);
        let mut rng = StdRng::seed_from_u64(seed);
        let t = doc.get_text("t");
        let mut next = 'A' as u32;
        let mut rejected_chars = std::collections::HashSet::new();
        for _ in 0..rng.gen_range(3..9) {
            let mut chars = vec![];
            for _ in 0..rng.gen_range(1..3) {
                let c = char::from_u32(next).unwrap();
                next += 1;
                let pos = rng.gen_range(0..=t.len_unicode());
                t.insert(pos, &c.to_string()).unwrap();
                chars.push(c);
            }
            if rng.gen_bool(0.35) && !doc.get_list("us").is_empty() {
                doc.get_list("us").delete(0, 1).unwrap();
                rejected_chars.extend(chars);
            }
            doc.commit();
        }
        let full = t.to_string();
        loop {
            match undo.undo() {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => assert!(is_unknown_err(&e), "seed {seed}: {e:?}"),
            }
        }
        let expected: String = full
            .chars()
            .filter(|c| rejected_chars.contains(c))
            .collect();
        assert_eq!(t.to_string(), expected, "seed {seed}: full {full:?}");
        while undo.redo().unwrap() {}
        assert_eq!(t.to_string(), full, "seed {seed}: after redo");
    }
}
