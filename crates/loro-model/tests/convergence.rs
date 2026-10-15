//! Randomized multi-peer test: view writes, `update_from` of mutated
//! projections, and random syncs must converge, and `update_from` must
//! round-trip and be idempotent at every step.

mod common;

use common::*;
use loro::{ExportMode, LoroDoc};
use loro_model::View;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const KEYS: usize = 8;

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.commit();
    b.commit();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
}

fn mutate(rng: &mut Rng, p: &mut Project) {
    for _ in 0..1 + rng.below(3) {
        match rng.below(6) {
            0 => p.title = format!("title-{}", rng.below(100)),
            1 => {
                let id = format!("k{}", rng.below(KEYS));
                if !p.todos.iter().any(|t| t.id == id) {
                    let at = rng.below(p.todos.len() + 1);
                    p.todos.insert(at, todo(&id, "new"));
                }
            }
            2 if !p.todos.is_empty() => {
                let i = rng.below(p.todos.len());
                p.todos.remove(i);
            }
            3 if !p.todos.is_empty() => {
                let i = rng.below(p.todos.len());
                p.todos[i].done = !p.todos[i].done;
                p.todos[i].title = format!("t{}", rng.below(100));
            }
            4 if p.todos.len() > 1 => {
                let i = rng.below(p.todos.len());
                let t = p.todos.remove(i);
                let j = rng.below(p.todos.len() + 1);
                p.todos.insert(j, t);
            }
            _ => {
                let at = rng.below(p.notes.chars().count() + 1);
                let mut chars: Vec<char> = p.notes.chars().collect();
                chars.insert(at, (b'a' + rng.below(26) as u8) as char);
                p.notes = chars.into_iter().collect();
            }
        }
    }
}

fn view_op(rng: &mut Rng, m: &ProjectModel) {
    let todos = m.todos();
    let id = format!("k{}", rng.below(KEYS));
    match rng.below(5) {
        0 => m.title().set(format!("v{}", rng.below(100))).unwrap(),
        1 => {
            if !todos.contains_key(&id).unwrap() {
                let at = rng.below(todos.len().unwrap() + 1);
                todos.insert(at, &todo(&id, "view")).unwrap();
            }
        }
        2 => {
            todos.remove(&id).unwrap();
        }
        3 => {
            if let Some(t) = todos.by_key(&id).unwrap() {
                t.done().set(!t.done().get().unwrap()).unwrap();
            }
        }
        _ => {
            let len = todos.len().unwrap();
            if todos.contains_key(&id).unwrap() {
                todos.move_to(&id, rng.below(len)).unwrap();
            }
        }
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let docs: Vec<LoroDoc> = (0..3)
        .map(|i| {
            let d = LoroDoc::new();
            d.set_peer_id(i + 1).unwrap();
            d
        })
        .collect();
    let models: Vec<ProjectModel> = docs
        .iter()
        .map(|d| ProjectModel::attach(d.get_map("project")))
        .collect();

    for _ in 0..steps {
        let p = rng.below(3);
        match rng.below(4) {
            0 | 1 => view_op(&mut rng, &models[p]),
            2 => {
                let mut dto: Project = models[p].read_as().unwrap_or_default();
                mutate(&mut rng, &mut dto);
                models[p].update_from(&dto).unwrap();
                assert_eq!(models[p].read_as::<Project>().unwrap(), dto, "seed {seed}");
                docs[p].commit();
                let before = docs[p].len_ops();
                models[p].update_from(&dto).unwrap();
                docs[p].commit();
                assert_eq!(docs[p].len_ops(), before, "not idempotent, seed {seed}");
            }
            _ => {
                let q = (p + 1 + rng.below(2)) % 3;
                sync(&docs[p], &docs[q]);
            }
        }
    }

    for _ in 0..2 {
        sync(&docs[0], &docs[1]);
        sync(&docs[1], &docs[2]);
    }
    let read: Vec<Option<Project>> = models.iter().map(|m| m.read_as().ok()).collect();
    assert_eq!(read[0], read[1], "seed {seed}");
    assert_eq!(read[1], read[2], "seed {seed}");
    for (m, r) in models.iter().zip(&read) {
        if let Some(r) = r {
            let ids: Vec<_> = r.todos.iter().map(|t| t.id.clone()).collect();
            assert_eq!(
                m.todos().keys().unwrap(),
                ids,
                "view order != projection, seed {seed}"
            );
            assert_eq!(m.todos().len().unwrap(), ids.len(), "seed {seed}");
        }
    }
    if let Some(p) = &read[0] {
        let mut ids: Vec<_> = p.todos.iter().map(|t| &t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            p.todos.len(),
            "projection has duplicate keys, seed {seed}"
        );
    }
}

#[test]
fn multi_peer_convergence() {
    let seeds: u64 = std::env::var("LORO_MODEL_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    for seed in 0..seeds {
        run(seed, 80);
    }
}
