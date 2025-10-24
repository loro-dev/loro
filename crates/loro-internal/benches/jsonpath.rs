use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod jsonpath {
    use std::hint::black_box;
    use super::*;
    use loro_internal::{ListHandler, LoroDoc, MapHandler};

    fn setup_large_doc(size: usize) -> LoroDoc {
        let doc = LoroDoc::new_auto_commit();
        let store = doc.get_map("store");
        let books = store.insert_container("books", ListHandler::new_detached()).unwrap();
        for i in 0..size {
            let book = books.insert_container(i, MapHandler::new_detached()).unwrap();
            book.insert("id", i as i64).unwrap();
            book.insert("title", format!("Book {}", i)).unwrap();
            book.insert("author", format!("Author {}", i % 10)).unwrap();
            book.insert("price", (10 + (i % 20)) as i64).unwrap();
            book.insert("available", i % 3 != 0).unwrap();
        }
        doc
    }

    pub fn index_selector(c: &mut Criterion) {
        let mut b = c.benchmark_group("jsonpath index selector");
        b.sample_size(20);
        for size in [100, 500].iter() {
            let doc = setup_large_doc(*size);
            b.bench_function(&format!("positive index ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[42].title").unwrap();
                    black_box(result);
                });
            });
            b.bench_function(&format!("negative index ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[-10].title").unwrap();
                    black_box(result);
                });
            });
        }
        b.finish();
    }

    pub fn slice_selector(c: &mut Criterion) {
        let mut b = c.benchmark_group("jsonpath slice selector");
        b.sample_size(20);
        for size in [100, 500].iter() {
            let doc = setup_large_doc(*size);
            b.bench_function(&format!("small slice 10 ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[0:10].title").unwrap();
                    black_box(result);
                });
            });
            b.bench_function(&format!("large slice half ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath(&format!("$.store.books[0:{}].title", size / 2)).unwrap();
                    black_box(result);
                });
            });
        }
        b.finish();
    }

    pub fn union_key_selector(c: &mut Criterion) {
        let mut b = c.benchmark_group("jsonpath union key selector");
        b.sample_size(20);
        for size in [100, 500].iter() {
            let doc = setup_large_doc(*size);
            b.bench_function(&format!("two keys single item ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[42]['title','author']").unwrap();
                    black_box(result);
                });
            });
            b.bench_function(&format!("three keys slice ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[0:10]['title','author','price']").unwrap();
                    black_box(result);
                });
            });
        }
        b.finish();
    }

    pub fn mixed_selectors(c: &mut Criterion) {
        let mut b = c.benchmark_group("jsonpath mixed selectors");
        b.sample_size(20);
        for size in [100, 500].iter() {
            let doc = setup_large_doc(*size);
            b.bench_function(&format!("index and slice mix ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[0,5:15,20].title").unwrap();
                    black_box(result);
                });
            });
        }
        b.finish();
    }

    pub fn real_world_patterns(c: &mut Criterion) {
        let mut b = c.benchmark_group("jsonpath real world patterns");
        b.sample_size(20);
        for size in [100, 500].iter() {
            let doc = setup_large_doc(*size);
            b.bench_function(&format!("get last 10 items ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[-10:].title").unwrap();
                    black_box(result);
                });
            });
            b.bench_function(&format!("pagination page 1 ({})", size), |bench| {
                bench.iter(|| {
                    let result = doc.jsonpath("$.store.books[0:20].title").unwrap();
                    black_box(result);
                });
            });
        }
        b.finish();
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(
    benches,
    jsonpath::index_selector,
    jsonpath::slice_selector,
    jsonpath::union_key_selector,
    jsonpath::mixed_selectors,
    jsonpath::real_world_patterns,
);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);

criterion_main!(benches);