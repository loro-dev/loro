//! Loom model of the last-handle drop racing a global registry scan. Separate
//! from `multi_thread_test` because it installs a process-wide global budget,
//! which would make the other models explore the registry too.
#[cfg(loom)]
mod loom_test {
    use loro::{global_memory_stats, set_global_memory_budget, GlobalMemoryBudget, LoroDoc};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn last_handle_drop_commits_while_the_registry_scans() {
        set_global_memory_budget(GlobalMemoryBudget {
            max_heap_bytes: Some(1 << 40),
            ..Default::default()
        });
        let mut builder = loom::model::Builder::new();
        builder.preemption_bound = Some(2);
        builder.check(|| {
            let doc = LoroDoc::new();
            let updates = Arc::new(AtomicUsize::new(0));
            let counter = updates.clone();
            doc.subscribe_local_update(Box::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            }))
            .detach();
            doc.get_text("t").insert(0, "x").unwrap();
            // The registry and `Arc` counts are not loom objects, so start the scan
            // before the drop; loom then explores every order of the two threads'
            // drop-gate and document-lock operations.
            let started = Arc::new(loom::sync::atomic::AtomicBool::new(false));
            let scanner = {
                let started = started.clone();
                loom::thread::spawn(move || {
                    started.store(true, Ordering::SeqCst);
                    let _ = global_memory_stats();
                })
            };
            while !started.load(Ordering::SeqCst) {
                loom::thread::yield_now();
            }
            drop(doc);
            assert_eq!(updates.load(Ordering::SeqCst), 1);
            scanner.join().unwrap();
            assert_eq!(updates.load(Ordering::SeqCst), 1);
        });
        set_global_memory_budget(GlobalMemoryBudget::default());
    }
}
