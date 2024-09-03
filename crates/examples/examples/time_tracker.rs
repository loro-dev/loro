use std::time::Instant;

use dev_utils::{get_mem_usage, ByteSize};
use loro::{LoroCounter, LoroDoc, LoroMap};

#[derive(Debug)]
struct NewProject {
    m: LoroMap,
    c: LoroCounter,
}

pub fn main() {
    let doc = LoroDoc::new();
    let projects = doc.get_map("projects");
    let n = 100;
    // ten years
    let total_time = 3600 * 24 * 365 * 10;
    let record_interval = 10;
    let mut all_projects: Vec<NewProject> = Vec::new();

    for i in 0..n {
        let project = projects
            .insert_container(&format!("project_{}", i), LoroMap::new())
            .unwrap();
        project.insert("name", format!("project_{}", i)).unwrap();
        let counter = project
            .insert_container("used_time", LoroCounter::new())
            .unwrap();
        all_projects.push(NewProject {
            m: project,
            c: counter,
        });
    }

    let times = total_time / record_interval;
    for i in 0..times {
        let project = rand::random::<usize>() % n;
        all_projects[project]
            .c
            .increment(record_interval as f64)
            .unwrap();
    }
    let mut total_time = 0.0;
    for project in all_projects.iter() {
        total_time += project.c.get();
    }

    println!("total_time: {}", total_time);
    println!("mem: {}", get_mem_usage());
    let snapshot = doc.export_fast_snapshot();
    println!("Snapshot Size {}", ByteSize(snapshot.len()));
    println!("mem: {}", get_mem_usage());
    let gc_snapshot = doc.export(loro::ExportMode::GcSnapshot(&doc.oplog_frontiers()));
    println!("GC Shallow Snapshot Size {}", ByteSize(gc_snapshot.len()));
    println!("mem: {}", get_mem_usage());

    let start = Instant::now();
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot);
    println!("Import Fast Snapshot Time: {:?}", start.elapsed());

    let start = Instant::now();
    let new_doc = LoroDoc::new();
    new_doc.import(&gc_snapshot);
    println!("Import GC Snapshot Time: {:?}", start.elapsed());
    let deep_value = new_doc.get_deep_value();
    println!(
        "Import GC Snapshot + Get Depp Value Time: {:?}",
        start.elapsed()
    );
    assert_eq!(deep_value, doc.get_deep_value());
}
