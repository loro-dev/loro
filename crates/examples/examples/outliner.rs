use dev_utils::{get_mem_usage, ByteSize};
use loro::LoroDoc;

pub fn main() {
    // Number of nodes
    let n = 100_000;
    // Number of moves per node
    let k = 5;
    // Max depth of the tree
    let max_depth = 10;
    let avg_peer_edits = 1000;
    let peers = (n * k / avg_peer_edits).max(1);
    println!("Number of nodes: {}", n);
    println!("Number of moves per node: {}", k);
    println!("Average number of peer edits: {}", avg_peer_edits);
    println!("Number of peers: {}", peers);

    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");
    let mut nodes = vec![];
    let mut depth = vec![0; n];
    for _ in 0..n {
        let node = tree.create(None).unwrap();
        nodes.push(node);
    }

    doc.commit();
    println!(
        "Memory usage after creating {} nodes: {}",
        n,
        get_mem_usage()
    );

    doc.compact_change_store();
    println!(
        "Memory usage after compacting change store: {}",
        get_mem_usage()
    );

    println!(
        "Updates size: {}",
        ByteSize(doc.export_from(&Default::default()).len())
    );
    let snapshot = doc.export_snapshot();
    println!("Snapshot size: {}", ByteSize(snapshot.len()));
    doc.with_oplog(|oplog| {
        println!(
            "Change store kv size: {}",
            ByteSize(oplog.change_store_kv_size())
        );
    });

    // Move nodes around
    for _ in (0..n * k).step_by(avg_peer_edits) {
        let new_doc = doc.fork();
        let new_tree = new_doc.get_tree("tree");

        for _ in 0..avg_peer_edits {
            let (mut i, mut j) = rand::random::<(usize, usize)>();
            i %= n;
            j %= n;
            while depth[j] > max_depth {
                j = rand::random::<usize>() % n;
            }

            if new_tree.mov_to(nodes[i], nodes[j], 0).is_ok() {
                depth[i] = depth[j] + 1;
            }
        }

        doc.import(&new_doc.export_from(&doc.oplog_vv())).unwrap();
    }

    let mem = get_mem_usage();
    println!("Memory usage after moving {} nodes: {}", n, mem);

    doc.compact_change_store();
    let mem_after_compact = get_mem_usage();
    println!(
        "Memory usage after compacting change store: {}",
        mem_after_compact
    );

    doc.free_diff_calculator();
    println!(
        "Memory usage after freeing diff calculator: {}",
        get_mem_usage()
    );
    doc.free_history_cache();
    println!(
        "Memory usage after freeing history cache: {}",
        get_mem_usage()
    );

    println!(
        "Updates size: {}",
        ByteSize(doc.export_from(&Default::default()).len())
    );
    let snapshot = doc.export_snapshot();
    println!("Snapshot size: {}", ByteSize(snapshot.len()));
    doc.compact_change_store();
    doc.with_oplog(|oplog| {
        println!(
            "Change store kv size: {}",
            ByteSize(oplog.change_store_kv_size())
        );
    });
}
