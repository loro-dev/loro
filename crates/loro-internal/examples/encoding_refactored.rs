use bench_utils::TextAction;
use criterion::black_box;
use loro_internal::refactor::loro::LoroApp;

fn main() {
    // let actions = bench_utils::get_automerge_actions();
    // {
    //     let loro = LoroApp::default();
    //     let text = loro.get_text("text");
    //     let mut txn = loro.txn().unwrap();

    //     for TextAction { pos, ins, del } in actions.iter() {
    //         text.delete(&mut txn, *pos, *del);
    //         text.insert(&mut txn, *pos, ins);
    //     }
    //     txn.commit().unwrap();
    //     let snapshot = loro.export_snapshot();
    //     let updates = loro.export_from(&Default::default());
    //     println!("\n");
    //     println!("Snapshot size={}", snapshot.len());
    //     println!("Updates size={}", updates.len());
    //     println!("\n");
    //     loro.diagnose_size();
    // }
    // println!("\n");
    // println!("\n");
    // println!("\n");
    // {
    //     println!("One Transaction Per Action");
    //     let loro = LoroApp::default();
    //     let text = loro.get_text("text");

    //     for TextAction { pos, ins, del } in actions.iter() {
    //         let mut txn = loro.txn().unwrap();
    //         text.delete(&mut txn, *pos, *del);
    //         text.insert(&mut txn, *pos, ins);
    //         txn.commit().unwrap();
    //     }
    //     let snapshot = loro.export_snapshot();
    //     let updates = loro.export_from(&Default::default());
    //     println!("\n");
    //     println!("Snapshot size={}", snapshot.len());
    //     println!("Updates size={}", updates.len());
    //     println!("\n");
    //     loro.diagnose_size();
    // }

    bench_decode();
}

fn bench_decode() {
    let actions = bench_utils::get_automerge_actions();
    {
        let loro = LoroApp::default();
        let text = loro.get_text("text");

        let mut txn = loro.txn().unwrap();
        let mut n = 0;
        for TextAction { pos, ins, del } in actions.iter() {
            if n % 10 == 0 {
                drop(txn);
                txn = loro.txn().unwrap();
            }
            n += 1;
            text.delete(&mut txn, *pos, *del);
            text.insert(&mut txn, *pos, ins);
        }
        txn.commit().unwrap();
        let snapshot = loro.export_snapshot();
        for _ in 0..100 {
            let loro = LoroApp::new();
            loro.import(black_box(&snapshot)).unwrap();
        }
    }
}
