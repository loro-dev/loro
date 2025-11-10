use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use arbitrary::Arbitrary;
    use arbitrary::Unstructured;
    use loro_internal::loro::ExportMode;
    use loro_internal::LoroDoc;
    use loro_internal::LoroValue;
    use rand::Rng;
    use rand::SeedableRng;

    pub fn many_list_containers(c: &mut Criterion) {
        #[derive(Arbitrary)]
        struct Action {
            container: u8,
            actor: u8,
            pos: u8,
            value: u8,
            sync: u8,
        }

        let mut rgn = rand::rngs::StdRng::seed_from_u64(0);
        let mut bytes = Vec::new();
        for _ in 0..10000 {
            bytes.push(rgn.gen::<u8>());
        }

        let mut gen = Unstructured::new(&bytes);
        let actions: [Action; 200] = gen.arbitrary().unwrap();
        let mut b = c.benchmark_group("10 list containers");
        b.sample_size(10);
        b.bench_function("sync random inserts to 10 list containers", |b| {
            b.iter(|| {
                let mut actors: Vec<_> = (0..10).map(|_| LoroDoc::default()).collect();
                for action in actions.iter() {
                    let len = actors.len();
                    let actor = &mut actors[action.actor as usize % len];
                    let container = action.container % 10;
                    if container % 2 == 0 {
                        let text = actor.get_text(container.to_string().as_str());
                        let mut txn = actor.txn().unwrap();
                        text.insert_with_txn(
                            &mut txn,
                            (action.pos as usize) % text.len_unicode().max(1),
                            action.value.to_string().as_str(),
                        )
                        .unwrap();
                    } else {
                        let list = actor.get_list(container.to_string().as_str());
                        let mut txn = actor.txn().unwrap();
                        list.insert_with_txn(
                            &mut txn,
                            (action.pos as usize) % list.len().max(1),
                            action.value.to_string().as_str().into(),
                        )
                        .unwrap();
                    }

                    let a = (action.actor as usize) % len;
                    let b = (action.sync as usize) % len;
                    if a != b {
                        let (a, b) = arref::array_mut_ref!(&mut actors, [a, b]);
                        a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
                            .unwrap();
                    }
                }

                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
                        .unwrap();
                }
                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [i, 0]);
                    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
                        .unwrap();
                }
            })
        });
    }

    pub fn many_actors(c: &mut Criterion) {
        let mut b = c.benchmark_group("many_actors");
        b.sample_size(10);
        b.bench_function("100 actors", |b| {
            b.iter(|| {
                let mut actors: Vec<_> = (0..100).map(|_| LoroDoc::default()).collect();
                for (i, actor) in actors.iter_mut().enumerate() {
                    let list = actor.get_list("list");
                    let value: LoroValue = i.to_string().into();
                    let mut txn = actor.txn().unwrap();
                    list.insert_with_txn(&mut txn, 0, value).unwrap();
                }

                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
                        .unwrap();
                }

                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
                        .unwrap();
                }
            })
        });
    }
}
pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, run::many_list_containers, run::many_actors);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
