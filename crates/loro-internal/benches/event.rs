use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod event {
    use super::*;
    
    use loro_internal::{ListHandler, LoroDoc};
    use std::sync::Arc;

    fn create_sub_container(handler: ListHandler, children_num: usize) -> Vec<ListHandler> {
        let mut ans = vec![];
        for idx in 0..children_num {
            let child_handler = handler
                .insert_container(idx, ListHandler::new_detached())
                .unwrap();
            ans.push(child_handler);
        }
        ans
    }

    pub fn resolved_container(c: &mut Criterion) {
        let mut b = c.benchmark_group("resolved");
        b.sample_size(10);
        b.bench_function("subContainer in event", |b| {
            let children_num = 80;
            let deep = 3;
            b.iter(|| {
                let mut loro = LoroDoc::default();
                loro.start_auto_commit();
                loro.subscribe_root(Arc::new(|_e| {}));
                let mut handlers = vec![loro.get_list("list")];
                for _ in 0..deep {
                    handlers = handlers
                        .into_iter()
                        .flat_map(|h| create_sub_container(h, children_num))
                        .collect();
                }
            })
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, event::resolved_container);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
