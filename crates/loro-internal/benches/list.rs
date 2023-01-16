use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use arbitrary::Arbitrary;
    use arbitrary::Unstructured;
    use loro_internal::LoroCore;
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
                let mut actors: Vec<_> = (0..10).map(|_| LoroCore::default()).collect();
                for action in actions.iter() {
                    let len = actors.len();
                    let actor = &mut actors[action.actor as usize % len];
                    let container = action.container % 10;
                    if container % 2 == 0 {
                        let mut text = actor.get_text(container.to_string().as_str());
                        text.insert(
                            actor,
                            (action.pos as usize) % text.len().max(1),
                            action.value.to_string().as_str(),
                        )
                        .unwrap();
                    } else {
                        let mut list = actor.get_list(container.to_string().as_str());
                        list.insert(
                            actor,
                            (action.pos as usize) % list.len().max(1),
                            action.value.to_string().as_str(),
                        )
                        .unwrap();
                    }

                    let a = (action.actor as usize) % len;
                    let b = (action.sync as usize) % len;
                    if a != b {
                        let (a, b) = arref::array_mut_ref!(&mut actors, [a, b]);
                        a.import(b.export(a.vv_cloned()));
                    }
                }

                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    a.import(b.export(a.vv_cloned()));
                }
                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [i, 0]);
                    a.import(b.export(a.vv_cloned()));
                }
            })
        });
    }

    pub fn many_actors(c: &mut Criterion) {
        let mut b = c.benchmark_group("many_actors");
        b.sample_size(10);
        b.bench_function("100 actors", |b| {
            b.iter(|| {
                let mut actors: Vec<_> = (0..100).map(|_| LoroCore::default()).collect();
                for (i, actor) in actors.iter_mut().enumerate() {
                    let mut list = actor.get_list("list");
                    let value: LoroValue = i.to_string().into();
                    list.insert(actor, 0, value).unwrap();
                }

                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    a.import(b.export(a.vv_cloned()));
                }

                for i in 1..actors.len() {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    b.import(a.export(b.vv_cloned()));
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
