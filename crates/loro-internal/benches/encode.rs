use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod sync {

    use super::*;
    use bench_utils::{get_automerge_actions, TextAction};
    use loro_internal::{cursor::PosType, encoding::ExportMode, LoroDoc};

    pub fn b4(c: &mut Criterion) {
        let actions = get_automerge_actions();
        let mut b = c.benchmark_group("encode_with_sync");
        b.sample_size(10);
        b.bench_function("update", |b| {
            b.iter(|| {
                let c1 = LoroDoc::new();
                c1.set_peer_id(1).unwrap();
                let c2 = LoroDoc::new();
                c2.set_peer_id(2).unwrap();
                let t1 = c1.get_text("text");
                let t2 = c2.get_text("text");
                for (i, action) in actions.iter().enumerate() {
                    if i > 2000 {
                        break;
                    }
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        let mut txn = c1.txn().unwrap();
                        t1.delete_with_txn(&mut txn, *pos, *del, PosType::Unicode)
                            .unwrap();
                        t1.insert_with_txn(&mut txn, *pos, ins, PosType::Unicode)
                            .unwrap();
                        txn.commit().unwrap();

                        let update = c1.export(ExportMode::updates(&c2.oplog_vv())).unwrap();
                        c2.import(&update).unwrap();
                    } else {
                        let mut txn = c2.txn().unwrap();
                        t2.delete_with_txn(&mut txn, *pos, *del, PosType::Unicode)
                            .unwrap();
                        t2.insert_with_txn(&mut txn, *pos, ins, PosType::Unicode)
                            .unwrap();
                        txn.commit().unwrap();
                        let update = c2.export(ExportMode::updates(&c1.oplog_vv())).unwrap();
                        c1.import(&update).unwrap();
                    }
                }
            })
        });
    }
}
#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use bench_utils::TextAction;
    use loro_internal::{cursor::PosType, encoding::ExportMode, LoroDoc};
    use std::hint::black_box;

    pub fn b4(c: &mut Criterion) {
        let loro = LoroDoc::default();
        let mut ran = false;
        let mut ensure_ran = || {
            if !ran {
                let actions = bench_utils::get_automerge_actions();
                let text = loro.get_text("text");
                for TextAction { pos, ins, del } in actions.iter() {
                    let mut txn = loro.txn().unwrap();
                    text.delete_with_txn(&mut txn, *pos, *del, PosType::Unicode)
                        .unwrap();
                    text.insert_with_txn(&mut txn, *pos, ins, PosType::Unicode)
                        .unwrap();
                }
                ran = true;
            }
        };

        let mut b = c.benchmark_group("encode");
        b.sample_size(10);
        b.bench_function("B4_encode_updates", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export(ExportMode::all_updates()).unwrap();
            })
        });
        b.bench_function("B4_decode_updates", |b| {
            ensure_ran();
            let buf = loro.export(ExportMode::all_updates()).unwrap();

            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import(&buf).unwrap();
            })
        });
        b.bench_function("B4_decode_updates detached mode", |b| {
            ensure_ran();
            let buf = loro.export(ExportMode::all_updates()).unwrap();

            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.detach();
                store2.import(&buf).unwrap();
            })
        });
        b.bench_function("B4_encode_snapshot", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export(ExportMode::Snapshot).unwrap();
            })
        });
        b.bench_function("B4_decode_snapshot", |b| {
            ensure_ran();
            let buf = loro.export(ExportMode::Snapshot).unwrap();
            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import(&buf).unwrap();
            })
        });

        b.bench_function("B4_encode_json_update", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export_json_updates(&Default::default(), &loro.oplog_vv(), true);
            })
        });
        b.bench_function("B4_decode_json_update", |b| {
            ensure_ran();
            let json = loro.export_json_updates(&Default::default(), &loro.oplog_vv(), true);
            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import_json_updates(json.clone()).unwrap();
            })
        });
    }

    fn build_issue_992_source_doc(paragraphs: usize) -> LoroDoc {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("codemirror");
        let mut chunks = Vec::with_capacity(paragraphs);
        for i in 0..paragraphs {
            chunks.push(format!(
                "# Section {i}\nParagraph {i} line A with repeated markdown content.\nParagraph {i} line B with **bold** and _italic_ markers.\n\n"
            ));
        }
        text.insert(0, &chunks.concat(), PosType::Unicode).unwrap();
        doc.commit_then_renew();

        for i in 0..6 {
            text.insert(
                text.len_unicode(),
                &format!("Tail block {i}: {}\n", "x".repeat(2048)),
                PosType::Unicode,
            )
            .unwrap();
            doc.commit_then_renew();
        }

        for i in 0..3 {
            let current = text.to_string();
            let next = current.replace(&format!("Section {i}"), &format!("Section {i} updated"))
                + &format!("\nreplace-round-{i}:{}", "y".repeat(1024));
            text.delete(0, text.len_unicode(), PosType::Unicode)
                .unwrap();
            text.insert(0, &next, PosType::Unicode).unwrap();
            doc.commit_then_renew();
        }

        doc
    }

    fn clone_doc(doc: &LoroDoc) -> LoroDoc {
        LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap()
    }

    fn build_issue_992_shallow_tail_doc(paragraphs: usize, tail_rounds: usize) -> LoroDoc {
        let source = build_issue_992_source_doc(paragraphs);
        let source = clone_doc(&source);
        let shallow_bytes = source
            .export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))
            .unwrap();
        let shallow_doc = LoroDoc::from_snapshot(&shallow_bytes).unwrap();
        let text = shallow_doc.get_text("codemirror");

        for i in 0..tail_rounds {
            let current = text.to_string();
            let next = current.replace(&format!("Section {i}"), &format!("Section {i} tail-{i}"))
                + &format!("\npost-shallow-tail-{i}:{}", "z".repeat(4096));
            text.delete(0, text.len_unicode(), PosType::Unicode)
                .unwrap();
            text.insert(0, &next, PosType::Unicode).unwrap();
            shallow_doc.commit_then_renew();
        }

        shallow_doc
    }

    pub fn issue_992_shallow_snapshot_tail(c: &mut Criterion) {
        let mut b = c.benchmark_group("encode_regression");
        b.sample_size(10);

        // This mirrors https://github.com/loro-dev/loro/issues/992: import a shallow
        // snapshot, perform a few full-document text replacements, then export snapshot.
        // The fixture is intentionally below the original report size so a regression is
        // visible without making the default benchmark prohibitively slow.
        let shallow_tail_doc = build_issue_992_shallow_tail_doc(1000, 2);
        b.bench_function("issue_992_shallow_tail_snapshot_export", |b| {
            b.iter(|| {
                black_box(shallow_tail_doc.export(ExportMode::Snapshot).unwrap().len());
            })
        });
    }
}

mod import {
    use criterion::{BatchSize, Criterion};
    use loro_common::LoroValue;
    use loro_internal::{cursor::PosType, encoding::ExportMode, LoroDoc};

    #[allow(dead_code)]
    pub fn causal_iter(c: &mut Criterion) {
        let mut b = c.benchmark_group("causal_iter");
        b.sample_size(10);
        b.bench_function("parallel_500", |b| {
            b.iter(|| {
                let c1 = LoroDoc::new();
                c1.set_peer_id(1).unwrap();
                let c2 = LoroDoc::new();
                c1.set_peer_id(2).unwrap();

                let text1 = c1.get_text("text");
                let text2 = c2.get_text("text");
                for _ in 0..500 {
                    text1
                        .insert_with_txn(&mut c1.txn().unwrap(), 0, "1", PosType::Unicode)
                        .unwrap();
                    text2
                        .insert_with_txn(&mut c2.txn().unwrap(), 0, "2", PosType::Unicode)
                        .unwrap();
                }

                let updates = c2.export(ExportMode::updates(&c1.oplog_vv())).unwrap();
                c1.import(&updates).unwrap()
            })
        });
    }

    struct BinaryImportFixture {
        base_update: Vec<u8>,
        incremental_update: Vec<u8>,
    }

    fn text_split_fixture(fragments: usize) -> BinaryImportFixture {
        const CHUNK_LEN: usize = 256;
        const PEER_A: u64 = 1;
        const PEER_B: u64 = 2;

        let doc_len = CHUNK_LEN * fragments;
        let doc_a = LoroDoc::new();
        doc_a.set_peer_id(PEER_A).unwrap();
        let text_a = doc_a.get_text("text");
        let mut txn = doc_a.txn().unwrap();
        text_a
            .insert_with_txn(&mut txn, 0, &"a".repeat(doc_len), PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let base_update = doc_a.export(ExportMode::all_updates()).unwrap();

        let doc_b = LoroDoc::new();
        doc_b.set_peer_id(PEER_B).unwrap();
        let text_b = doc_b.get_text("text");
        doc_b.import(&base_update).unwrap();
        let base_vv = doc_b.oplog_vv();
        let mut txn = doc_b.txn().unwrap();
        for i in 0..(fragments - 1) {
            let pos = (i + 1) * CHUNK_LEN + i;
            text_b
                .insert_with_txn(&mut txn, pos, "x", PosType::Unicode)
                .unwrap();
        }
        txn.commit().unwrap();
        let incremental_update = doc_b.export(ExportMode::updates(&base_vv)).unwrap();

        BinaryImportFixture {
            base_update,
            incremental_update,
        }
    }

    fn list_diff_fixture(items: usize, inserts: usize) -> BinaryImportFixture {
        const PEER_A: u64 = 11;
        const PEER_B: u64 = 12;

        let doc_a = LoroDoc::new();
        doc_a.set_peer_id(PEER_A).unwrap();
        let list_a = doc_a.get_list("list");
        let mut txn = doc_a.txn().unwrap();
        for i in 0..items {
            list_a
                .insert_with_txn(&mut txn, i, LoroValue::I64(i as i64))
                .unwrap();
        }
        txn.commit().unwrap();
        let base_update = doc_a.export(ExportMode::all_updates()).unwrap();

        let doc_b = LoroDoc::new();
        doc_b.set_peer_id(PEER_B).unwrap();
        let list_b = doc_b.get_list("list");
        doc_b.import(&base_update).unwrap();
        let base_vv = doc_b.oplog_vv();
        let mut txn = doc_b.txn().unwrap();
        for i in 0..inserts {
            let len = items + i;
            let pos = (i * 7) % len;
            list_b
                .insert_with_txn(&mut txn, pos, LoroValue::I64(-((i as i64) + 1)))
                .unwrap();
        }
        txn.commit().unwrap();
        let incremental_update = doc_b.export(ExportMode::updates(&base_vv)).unwrap();

        BinaryImportFixture {
            base_update,
            incremental_update,
        }
    }

    fn import_attached(fixture: &BinaryImportFixture) {
        let doc = LoroDoc::new();
        doc.import(&fixture.base_update).unwrap();
        doc.import(&fixture.incremental_update).unwrap();
    }

    fn import_detached(fixture: &BinaryImportFixture) {
        let doc = LoroDoc::new();
        doc.import(&fixture.base_update).unwrap();
        doc.detach();
        doc.import(&fixture.incremental_update).unwrap();
    }

    fn checkout_after_detached_import(fixture: &BinaryImportFixture) {
        let doc = LoroDoc::new();
        doc.import(&fixture.base_update).unwrap();
        doc.detach();
        doc.import(&fixture.incremental_update).unwrap();
        doc.checkout_to_latest();
    }

    #[allow(dead_code)]
    pub fn import_regression(c: &mut Criterion) {
        let text_fixture = text_split_fixture(1024);
        let list_fixture = list_diff_fixture(4096, 1024);

        let mut b = c.benchmark_group("import_regression");
        b.sample_size(10);

        b.bench_function("text_split_attached_import_1024", |b| {
            b.iter_batched(|| &text_fixture, import_attached, BatchSize::SmallInput)
        });
        b.bench_function("text_split_detached_import_1024", |b| {
            b.iter_batched(|| &text_fixture, import_detached, BatchSize::SmallInput)
        });
        b.bench_function("text_split_checkout_1024", |b| {
            b.iter_batched(
                || &text_fixture,
                checkout_after_detached_import,
                BatchSize::SmallInput,
            )
        });
        b.bench_function("list_attached_import_4096x1024", |b| {
            b.iter_batched(|| &list_fixture, import_attached, BatchSize::SmallInput)
        });
        b.bench_function("list_checkout_4096x1024", |b| {
            b.iter_batched(
                || &list_fixture,
                checkout_after_detached_import,
                BatchSize::SmallInput,
            )
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(
    benches,
    run::b4,
    run::issue_992_shallow_snapshot_tail,
    sync::b4,
    import::causal_iter,
    import::import_regression
);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
