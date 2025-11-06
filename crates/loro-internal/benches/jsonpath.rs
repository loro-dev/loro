use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "test_utils")]
mod jsonpath {
    use criterion::Criterion;
    use loro_internal::{ListHandler, LoroDoc, MapHandler};
    use std::hint::black_box;

    // Sizes to test: small, medium, large
    const SIZES: [usize; 3] = [100, 500, 2000];

    /// Setup a large document with:
    /// - books[0..size] with title, author, price, available
    /// - '1984' by George Orwell every ~100 items
    /// - Nested structure under $.catalog.fiction.books for recursive tests
    fn setup_large_doc(size: usize) -> LoroDoc {
        let doc = LoroDoc::new_auto_commit();
        let store = doc.get_map("store");
        let books = store
            .insert_container("books", ListHandler::new_detached())
            .unwrap();

        for i in 0..size {
            let book = books
                .insert_container(i, MapHandler::new_detached())
                .unwrap();
            let title = if i % 100 == 42 {
                "1984"
            } else {
                &format!("Book {}", i)
            };
            let author = if i % 100 == 42 {
                "George Orwell"
            } else {
                &format!("Author {}", i % 10)
            };
            let price = 5.0 + (i % 25) as f64;
            let available = i % 4 != 0;

            book.insert("title", title).unwrap();
            book.insert("author", author).unwrap();
            book.insert("price", price).unwrap();
            book.insert("available", available).unwrap();
        }

        // Add nested structure for $..title and recursive filters
        let catalog = doc.get_map("catalog");
        let fiction = catalog
            .insert_container("fiction", MapHandler::new_detached())
            .unwrap();
        let nested_books = fiction
            .insert_container("books", ListHandler::new_detached())
            .unwrap();
        for i in 0..10 {
            let b = nested_books
                .insert_container(i, MapHandler::new_detached())
                .unwrap();
            b.insert("title", format!("Nested {}", i)).unwrap();
            b.insert("price", 12.0 + i as f64).unwrap();
        }

        doc
    }

    /// Benchmark a single JSONPath query across all sizes
    fn bench_pattern<F>(c: &mut Criterion, group_name: &str, query: &str, setup: F)
    where
        F: Fn(usize) -> LoroDoc,
    {
        let mut group = c.benchmark_group(group_name);
        group.sample_size(if group_name.contains("large") { 20 } else { 50 });

        for &size in &SIZES {
            let doc = setup(size);
            let name = format!("{} (size={})", query, size);
            group.bench_function(&name, |b| {
                b.iter(|| {
                    let result = doc.jsonpath(query).unwrap();
                    black_box(result);
                })
            });
        }
        group.finish();
    }

    pub fn child_selector(c: &mut Criterion) {
        bench_pattern(
            c,
            "Child Selector",
            "$.store.books[0].title",
            setup_large_doc,
        );
    }

    pub fn wildcard(c: &mut Criterion) {
        bench_pattern(c, "Wildcard", "$.store.books[*].title", setup_large_doc);
    }

    pub fn recursive_descent(c: &mut Criterion) {
        bench_pattern(c, "Recursive Descent", "$..title", setup_large_doc);
    }

    pub fn quoted_keys(c: &mut Criterion) {
        bench_pattern(
            c,
            "Quoted Keys",
            "$.store['books'][0]['title']",
            setup_large_doc,
        );
    }

    pub fn string_filter(c: &mut Criterion) {
        bench_pattern(
            c,
            "String Filter",
            "$.store.books[?(@.title == '1984')].title",
            setup_large_doc,
        );
    }

    pub fn logical_operator(c: &mut Criterion) {
        bench_pattern(
            c,
            "Logical Operator",
            "$.store.books[?(@.author == 'George Orwell' && @.price < 10)].title",
            setup_large_doc,
        );
    }

    pub fn union_operation(c: &mut Criterion) {
        bench_pattern(
            c,
            "Union Operation",
            "$.store.books[0,2].title",
            setup_large_doc,
        );
    }

    pub fn slice_operation(c: &mut Criterion) {
        let mut group = c.benchmark_group("Slice Operation");

        for &size in &SIZES {
            let doc = setup_large_doc(size);

            // Small slice
            group.bench_function(format!("$.store.books[0:3].title (size={})", size), |b| {
                b.iter(|| {
                    let result = doc.jsonpath("$.store.books[0:3].title").unwrap();
                    black_box(result);
                })
            });

            // Large slice (half)
            let half = size / 2;
            group.bench_function(
                format!("$.store.books[0:{}].title (size={})", half, size),
                |b| {
                    b.iter(|| {
                        let result = doc
                            .jsonpath(&format!("$.store.books[0:{}].title", half))
                            .unwrap();
                        black_box(result);
                    })
                },
            );

            // Negative slice
            group.bench_function(format!("$.store.books[-10:].title (size={})", size), |b| {
                b.iter(|| {
                    let result = doc.jsonpath("$.store.books[-10:].title").unwrap();
                    black_box(result);
                })
            });
        }
        group.finish();
    }

    pub fn complex_filter(c: &mut Criterion) {
        bench_pattern(
            c,
            "Complex Filter",
            "$.store.books[?(@.price >= 10 && @.available == true && @.title contains '1984')].title",
            setup_large_doc,
        );
    }

    pub fn recursive_mapped_filter(c: &mut Criterion) {
        bench_pattern(
            c,
            "Recursive Mapped Filter",
            "$.store.books[?(@.price > 10)].title",
            setup_large_doc,
        );
    }

    pub fn recursive_filter(c: &mut Criterion) {
        bench_pattern(
            c,
            "Recursive Filter",
            "$..[?(@.price > 10)].title",
            setup_large_doc,
        );
    }

    // === Legacy / Extra: Keep your original ones (now size-scaled) ===

    pub fn index_selector(c: &mut Criterion) {
        let mut group = c.benchmark_group("Index Selector");
        group.sample_size(50);

        for &size in &SIZES {
            let doc = setup_large_doc(size);
            let idx = (size as i64 / 2).min(42); // Safe index
            group.bench_function(format!("positive index {} (size={})", idx, size), |b| {
                b.iter(|| {
                    let result = doc
                        .jsonpath(&format!("$.store.books[{}].title", idx))
                        .unwrap();
                    black_box(result);
                })
            });

            group.bench_function(format!("negative index -10 (size={})", size), |b| {
                b.iter(|| {
                    let result = doc.jsonpath("$.store.books[-10].title").unwrap();
                    black_box(result);
                })
            });
        }
        group.finish();
    }

    pub fn union_key_selector(c: &mut Criterion) {
        let mut group = c.benchmark_group("Union Key Selector");
        group.sample_size(50);

        for &size in &SIZES {
            let doc = setup_large_doc(size);
            group.bench_function(format!("two keys single item (size={})", size), |b| {
                b.iter(|| {
                    let result = doc.jsonpath("$.store.books[42]['title','author']").unwrap();
                    black_box(result);
                })
            });

            group.bench_function(format!("three keys slice (size={})", size), |b| {
                b.iter(|| {
                    let result = doc
                        .jsonpath("$.store.books[0:10]['title','author','price']")
                        .unwrap();
                    black_box(result);
                })
            });
        }
        group.finish();
    }

    pub fn mixed_selectors(c: &mut Criterion) {
        bench_pattern(
            c,
            "Mixed Selectors",
            "$.store.books[0,5:15,20].title",
            setup_large_doc,
        );
    }

    pub fn real_world_patterns(c: &mut Criterion) {
        let mut group = c.benchmark_group("Real World Patterns");
        group.sample_size(50);

        for &size in &SIZES {
            let doc = setup_large_doc(size);
            group.bench_function(format!("last 10 items (size={})", size), |b| {
                b.iter(|| {
                    let result = doc.jsonpath("$.store.books[-10:].title").unwrap();
                    black_box(result);
                })
            });

            group.bench_function(format!("pagination page 1 (size={})", size), |b| {
                b.iter(|| {
                    let result = doc.jsonpath("$.store.books[0:20].title").unwrap();
                    black_box(result);
                })
            });
        }
        group.finish();
    }

    // === Master Group ===
    pub fn all_benches(c: &mut Criterion) {
        child_selector(c);
        wildcard(c);
        recursive_descent(c);
        quoted_keys(c);
        string_filter(c);
        logical_operator(c);
        union_operation(c);
        slice_operation(c);
        complex_filter(c);
        recursive_mapped_filter(c);
        recursive_filter(c);
        index_selector(c);
        union_key_selector(c);
        mixed_selectors(c);
        real_world_patterns(c);
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, jsonpath::all_benches);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);

criterion_main!(benches);
