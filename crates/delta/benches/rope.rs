use std::ops::Range;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use delta::text_delta::TextDelta;

fn rope_benchmarks(c: &mut Criterion) {
    static KB: usize = 1024;

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

    // let mut group = c.benchmark_group("push");
    // for size in sizes.iter() {
    //     group.throughput(Throughput::Bytes(*size as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
    //         let text = generate_random_text(rng.clone(), *size);

    //         b.iter(|| {
    //             let mut rope = Rope::new();
    //             for _ in 0..10 {
    //                 rope.push(&text);
    //             }
    //         });
    //     });
    // }
    // group.finish();

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
