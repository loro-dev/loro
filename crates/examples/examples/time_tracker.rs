use dev_utils::{get_mem_usage, ByteSize};
use loro::{CommitOptions, LoroCounter, LoroDoc, LoroMap};

#[derive(Debug)]
struct NewProject {
    // m: LoroMap,
    c: LoroCounter,
}

pub fn main() {
    let doc = LoroDoc::new();
    doc.set_record_timestamp(true);
    doc.set_change_merge_interval(1000 * 600);
    let projects = doc.get_map("projects");
    let n = 100;
    // ten years
    let total_time = 3600 * 24 * 365 * 10;
    let record_interval = 10;
    let mut all_projects: Vec<NewProject> = Vec::new();

    for i in 0..n {
        let project = projects
            .insert_container(&format!("project_{i}"), LoroMap::new())
            .unwrap();
        project.insert("name", format!("project_{i}")).unwrap();
        let counter = project
            .insert_container("used_time", LoroCounter::new())
            .unwrap();
        all_projects.push(NewProject {
            // m: project,
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
        doc.commit_with(CommitOptions::new().timestamp(i * 1000 * 10));
    }

    let mut total_time = 0.0;
    for project in all_projects.iter() {
        total_time += project.c.get();
    }

    println!("total_time: {total_time}");
    println!("mem: {}", get_mem_usage());
    let snapshot = doc.export(loro::ExportMode::Snapshot);
    println!("Snapshot Size {}", ByteSize(snapshot.unwrap().len()));
    println!("mem: {}", get_mem_usage());
    let shallow_snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    println!(
        "GC Shallow Snapshot Size {}",
        ByteSize(shallow_snapshot.unwrap().len())
    );
    println!("mem: {}", get_mem_usage());

    examples::utils::bench_fast_snapshot(&doc);
}
