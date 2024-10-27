use std::time::Instant;

use examples::list::{append_n, prepend_n, random_delete, random_insert, random_move, random_set};
use loro::{LoroDoc, ToJson};
use tabled::{settings::Style, Table, Tabled};

#[derive(Tabled)]
struct BenchResult {
    task: &'static str,
    snapshot_size: usize,
    updates_size: usize,
    apply_duration: f64,
    encode_snapshot_duration: f64,
    encode_udpate_duration: f64,
    decode_snapshot_duration: f64,
    decode_update_duration: f64,
    doc_json_size: usize,
}

pub fn main() {
    let results = [
        run("[Movable List] Append x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_movable_list("list");
            append_n(&mut list, 10_000);
            doc
        }),
        run("[Movable List] Prepend x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_movable_list("list");
            prepend_n(&mut list, 10_000);
            doc
        }),
        run("[Movable List] Random Insert x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_movable_list("list");
            random_insert(&mut list, 10_000, 999);
            list.mov(0, 1).unwrap();
            doc
        }),
        run("[Movable List] Random Insert&Delete x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_movable_list("list");
            random_insert(&mut list, 10_000, 999);
            random_delete(&mut list, 10_000, 999);
            doc
        }),
        run("[Movable List] Random Insert&Delete x 100_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_movable_list("list");
            random_insert(&mut list, 100_000, 999);
            random_delete(&mut list, 100_000, 999);
            doc
        }),
        run("[Movable List] Collab Insert x 100_000", || {
            let doc_a = LoroDoc::new();
            let mut list_a = doc_a.get_movable_list("list");
            let doc_b = LoroDoc::new();
            let mut list_b = doc_b.get_movable_list("list");
            for i in 0..1000 {
                random_insert(&mut list_a, 100, i);
                random_insert(&mut list_b, 100, i);
                doc_a
                    .import(
                        &doc_b
                            .export(loro::ExportMode::updates(&doc_a.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
                doc_b
                    .import(
                        &doc_a
                            .export(loro::ExportMode::updates(&doc_b.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
            }

            doc_a
        }),
        run("[List] Append x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_list("list");
            append_n(&mut list, 10_000);
            doc
        }),
        run("[List] Prepend x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_list("list");
            prepend_n(&mut list, 10_000);
            doc
        }),
        run("[List] Random Insert x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_list("list");
            random_insert(&mut list, 10_000, 999);
            doc
        }),
        run("[List] Random Insert&Delete x 10_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_list("list");
            random_insert(&mut list, 10_000, 999);
            random_delete(&mut list, 10_000, 999);
            doc
        }),
        run("[List] Random Insert&Delete x 100_000", || {
            let doc = LoroDoc::new();
            let mut list = doc.get_list("list");
            random_insert(&mut list, 100_000, 999);
            random_delete(&mut list, 100_000, 999);
            doc
        }),
        run("[List] Collab Insert x 100_000", || {
            let doc_a = LoroDoc::new();
            let mut list_a = doc_a.get_list("list");
            let doc_b = LoroDoc::new();
            let mut list_b = doc_b.get_list("list");
            for i in 0..1000 {
                random_insert(&mut list_a, 100, i);
                random_insert(&mut list_b, 100, i);
                doc_a
                    .import(
                        &doc_b
                            .export(loro::ExportMode::updates(&doc_a.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
                doc_b
                    .import(
                        &doc_a
                            .export(loro::ExportMode::updates(&doc_b.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
            }

            doc_a
        }),
        run("[Movable List] Collab Set x 100_000", || {
            let doc_a = LoroDoc::new();
            let mut list_a = doc_a.get_movable_list("list");
            let doc_b = LoroDoc::new();
            let mut list_b = doc_b.get_movable_list("list");
            random_insert(&mut list_a, 1000, 0);
            random_insert(&mut list_b, 1000, 0);
            doc_a
                .import(
                    &doc_b
                        .export(loro::ExportMode::updates(&doc_a.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            doc_b
                .import(
                    &doc_a
                        .export(loro::ExportMode::updates(&doc_b.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            for i in 0..1000 {
                random_set(&mut list_a, 100, i);
                random_set(&mut list_b, 100, i);
                doc_a
                    .import(
                        &doc_b
                            .export(loro::ExportMode::updates(&doc_a.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
                doc_b
                    .import(
                        &doc_a
                            .export(loro::ExportMode::updates(&doc_b.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
            }

            doc_a
        }),
        run("[Movable List] Collab Move x 100_000", || {
            let doc_a = LoroDoc::new();
            let mut list_a = doc_a.get_movable_list("list");
            let doc_b = LoroDoc::new();
            let mut list_b = doc_b.get_movable_list("list");
            random_insert(&mut list_a, 1000, 0);
            random_insert(&mut list_b, 1000, 0);
            doc_a
                .import(
                    &doc_b
                        .export(loro::ExportMode::updates(&doc_a.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            doc_b
                .import(
                    &doc_a
                        .export(loro::ExportMode::updates(&doc_b.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            for i in 0..1000 {
                random_move(&mut list_a, 100, i);
                random_move(&mut list_b, 100, i);
                doc_a
                    .import(
                        &doc_b
                            .export(loro::ExportMode::updates(&doc_a.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
                doc_b
                    .import(
                        &doc_a
                            .export(loro::ExportMode::updates(&doc_b.oplog_vv()))
                            .unwrap(),
                    )
                    .unwrap();
            }

            doc_a
        }),
    ];

    let mut table = Table::new(results);
    let style = Style::markdown();
    table.with(style);
    println!("{table}");
}

fn run(name: &'static str, apply_task: impl FnOnce() -> LoroDoc) -> BenchResult {
    let start = Instant::now();
    let doc = apply_task();
    let apply_duration = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let snapshot = doc.export(loro::ExportMode::Snapshot).unwrap();
    let encode_snapshot_duration = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let updates = doc.export(loro::ExportMode::all_updates()).unwrap();
    let encode_update_duration = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot).unwrap();
    let decode_snapshot_duration = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let new_doc = LoroDoc::new();
    new_doc.import(&updates).unwrap();
    let decode_update_duration = start.elapsed().as_secs_f64() * 1000.;

    let doc_json_size = doc.get_deep_value().to_json().len();

    BenchResult {
        task: name,
        snapshot_size: snapshot.len(),
        updates_size: updates.len(),
        apply_duration,
        encode_snapshot_duration,
        encode_udpate_duration: encode_update_duration,
        decode_snapshot_duration,
        decode_update_duration,
        doc_json_size,
    }
}
