use arbitrary::Unstructured;
use criterion::{criterion_group, criterion_main, Criterion};
use loro_core::fuzz::test_multi_sites;
use loro_core::fuzz::Action;
use rand::Rng;
use rand::SeedableRng;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut rgn = rand::rngs::StdRng::seed_from_u64(0);
    let mut bytes = Vec::new();
    for _ in 0..1000 {
        bytes.push(rgn.gen::<u8>());
    }

    let mut gen = Unstructured::new(&bytes);
    let actions = gen.arbitrary::<[Action; 200]>().unwrap();
    c.bench_function("random text edit 2 sites", |b| {
        b.iter(|| test_multi_sites(2, actions.clone().into()))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
