use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod pending {
    use super::*;
    use bench_utils::TextAction;
    use loro_internal::{LoroDoc, VersionVector};

    pub fn b4(c: &mut Criterion) {
        let mut b = c.benchmark_group("B4 pending decode");
        b.sample_size(10);
        b.bench_function("detached mode", |b| {
            let loro = LoroDoc::default();
            let mut latest_vv = VersionVector::default();
            let mut updates = vec![];
            let actions = bench_utils::get_automerge_actions();
            let action_length = actions.len();
            let text = loro.get_text("text");
            for chunks in actions.chunks(action_length / 5) {
                for TextAction { pos, ins, del } in chunks {
                    let mut txn = loro.txn().unwrap();
                    text.delete(&mut txn, *pos, *del).unwrap();
                    text.insert(&mut txn, *pos, ins).unwrap();
                    updates.push(loro.export_from(&latest_vv));
                    latest_vv = loro.oplog_vv();
                }
            }
            updates.reverse();
            b.iter(|| {
                let mut store2 = LoroDoc::default();
                store2.detach();
                for update in updates.iter() {
                    store2.import(update).unwrap();
                }
            })
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, pending::b4);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
