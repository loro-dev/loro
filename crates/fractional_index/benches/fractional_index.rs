use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(not(feature = "jitter"))]
pub fn criterion_benchmark(c: &mut Criterion) {
    use criterion::{AxisScale, BenchmarkId, PlotConfiguration};
    use fraction_index::FractionalIndex as MyIndex;
    use fractional_index::FractionalIndex;
    let mut group = c.benchmark_group("FractionalIndex");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    group.sample_size(10);
    let base = 10u64;
    for i in 2..=5 {
        group.bench_with_input(
            BenchmarkId::new("new_after-im", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    let mut after = MyIndex::default();
                    for _ in 0..*i {
                        after = MyIndex::new_after(&after);
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("new_after", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    let mut after = FractionalIndex::default();
                    for _ in 0..*i {
                        after = FractionalIndex::new_after(&after);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("new_before-im", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    let mut before = MyIndex::default();
                    for _ in 0..*i {
                        before = MyIndex::new_before(&before);
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("new_before", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    let mut before = FractionalIndex::default();
                    for _ in 0..*i {
                        before = FractionalIndex::new_before(&before);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("new_between-im", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    let mut before = MyIndex::default();
                    let mut after = MyIndex::new_after(&before);
                    for i in 0..*i {
                        if i % 2 == 0 {
                            let index = MyIndex::new_between(&before, &after).unwrap();
                            before = index;
                        } else {
                            let index = MyIndex::new_between(&before, &after).unwrap();
                            after = index;
                        }
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("new_between", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    let mut before = FractionalIndex::default();
                    let mut after = FractionalIndex::new_after(&before);
                    for i in 0..*i {
                        if i % 2 == 0 {
                            let index = FractionalIndex::new_between(&before, &after).unwrap();
                            before = index;
                        } else {
                            let index = FractionalIndex::new_between(&before, &after).unwrap();
                            after = index;
                        }
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("new_evenly", base.pow(i)),
            &base.pow(i),
            |b, i| {
                b.iter(|| {
                    FractionalIndex::generate_n_evenly(None, None, *i as usize);
                });
            },
        );
    }
    group.finish();

    c.bench_function("evenly 10^5", |b| {
        b.iter(|| {
            FractionalIndex::generate_n_evenly(None, None, 100000);
        });
    });
}

#[cfg(feature = "jitter")]
fn criterion_benchmark(_: &mut Criterion) {}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
