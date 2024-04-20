use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use delta::text_delta::TextDelta;
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

fn generate_random_text(mut rng: StdRng, text_len: usize) -> String {
    RandomCharIter::new(&mut rng).take(text_len).collect()
}

fn rope_benchmarks(c: &mut Criterion) {
    static SEED: u64 = 9999;
    static KB: usize = 1024;

    let rng = StdRng::seed_from_u64(SEED);
    let sizes = [4 * KB, 64 * KB, 256 * KB];

    let mut group = c.benchmark_group("insert");
    for size in sizes.iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut rope = TextDelta::new();
                for i in 0..*size {
                    let index = i * 3 / 4;
                    rope.insert_str(index, "n");
                }
            });
        });
    }
    group.finish();

    let mut group = c.benchmark_group("push");
    for size in sizes.iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let text = generate_random_text(rng.clone(), *size);

            b.iter(|| {
                let mut rope = TextDelta::new();
                for _ in 0..10 {
                    rope.push_str_insert(&text);
                }
            });
        });
    }
    group.finish();

    // let mut group = c.benchmark_group("append");
    // for size in sizes.iter() {
    //     group.throughput(Throughput::Bytes(*size as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
    //         let mut random_ropes = Vec::new();
    //         for _ in 0..5 {
    //             random_ropes.push(generate_random_rope(rng.clone(), *size));
    //         }

    //         b.iter(|| {
    //             let mut rope_b = Rope::new();
    //             for rope in &random_ropes {
    //                 rope_b.append(rope.clone())
    //             }
    //         });
    //     });
    // }
    // group.finish();

    // let mut group = c.benchmark_group("slice");
    // for size in sizes.iter() {
    //     group.throughput(Throughput::Bytes(*size as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
    //         let rope = generate_random_rope(rng.clone(), *size);

    //         b.iter_batched(
    //             || generate_random_rope_ranges(rng.clone(), &rope),
    //             |ranges| {
    //                 for range in ranges.iter() {
    //                     rope.slice(range.clone());
    //                 }
    //             },
    //             BatchSize::SmallInput,
    //         );
    //     });
    // }
    // group.finish();

    // let mut group = c.benchmark_group("bytes_in_range");
    // for size in sizes.iter() {
    //     group.throughput(Throughput::Bytes(*size as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
    //         let rope = generate_random_rope(rng.clone(), *size);

    //         b.iter_batched(
    //             || generate_random_rope_ranges(rng.clone(), &rope),
    //             |ranges| {
    //                 for range in ranges.iter() {
    //                     let bytes = rope.bytes_in_range(range.clone());
    //                     assert!(bytes.into_iter().count() > 0);
    //                 }
    //             },
    //             BatchSize::SmallInput,
    //         );
    //     });
    // }
    // group.finish();

    // let mut group = c.benchmark_group("chars");
    // for size in sizes.iter() {
    //     group.throughput(Throughput::Bytes(*size as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
    //         let rope = generate_random_rope(rng.clone(), *size);

    //         b.iter_with_large_drop(|| {
    //             let chars = rope.chars().count();
    //             assert!(chars > 0);
    //         });
    //     });
    // }
    // group.finish();

    // let mut group = c.benchmark_group("clip_point");
    // for size in sizes.iter() {
    //     group.throughput(Throughput::Bytes(*size as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
    //         let rope = generate_random_rope(rng.clone(), *size);

    //         b.iter_batched(
    //             || generate_random_rope_points(rng.clone(), &rope),
    //             |offsets| {
    //                 for offset in offsets.iter() {
    //                     black_box(rope.clip_point(*offset, Bias::Left));
    //                     black_box(rope.clip_point(*offset, Bias::Right));
    //                 }
    //             },
    //             BatchSize::SmallInput,
    //         );
    //     });
    // }
    // group.finish();
}

criterion_group!(benches, rope_benchmarks);
criterion_main!(benches);

pub struct RandomCharIter<T: Rng> {
    rng: T,
    simple_text: bool,
}

impl<T: Rng> RandomCharIter<T> {
    pub fn new(rng: T) -> Self {
        Self {
            rng,
            simple_text: std::env::var("SIMPLE_TEXT").map_or(false, |v| !v.is_empty()),
        }
    }

    pub fn with_simple_text(mut self) -> Self {
        self.simple_text = true;
        self
    }
}

impl<T: Rng> Iterator for RandomCharIter<T> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.simple_text {
            return if self.rng.gen_range(0..100) < 5 {
                Some('\n')
            } else {
                Some(self.rng.gen_range(b'a'..b'z' + 1).into())
            };
        }

        match self.rng.gen_range(0..100) {
            // whitespace
            0..=19 => [' ', '\n', '\r', '\t'].choose(&mut self.rng).copied(),
            // two-byte greek letters
            20..=32 => char::from_u32(self.rng.gen_range(('α' as u32)..('ω' as u32 + 1))),
            // // three-byte characters
            33..=45 => ['✋', '✅', '❌', '❎', '⭐']
                .choose(&mut self.rng)
                .copied(),
            // // four-byte characters
            46..=58 => ['🍐', '🏀', '🍗', '🎉'].choose(&mut self.rng).copied(),
            // ascii letters
            _ => Some(self.rng.gen_range(b'a'..b'z' + 1).into()),
        }
    }
}
