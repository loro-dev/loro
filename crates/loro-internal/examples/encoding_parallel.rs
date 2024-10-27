use bench_utils::{get_automerge_actions, TextAction};
use loro_internal::LoroDoc;

// #[allow(dead_code)]
// fn parallel() {
//     let mut rng: StdRng = SeedableRng::seed_from_u64(1);

//     let actions = get_automerge_actions();
//     let mut loro = LoroCore::new(Default::default(), Some(1));
//     let mut loro_b = LoroCore::new(Default::default(), Some(2));
//     let mut i = 0;
//     for TextAction { pos, ins, del } in actions.iter() {
//         i += 1;
//         if i > 1000 {
//             break;
//         }
//         let pos = *pos;
//         let del = *del;
//         let mut text = loro.get_text("text");
//         text.delete(&loro, pos, del).unwrap();
//         text.insert(&loro, pos, ins).unwrap();
//         let mut text = loro_b.get_text("text");
//         let r = rng.gen_range(1..11);
//         for _ in 0..r {
//             text.delete(&loro_b, pos, del).unwrap();
//             text.insert(&loro_b, pos, ins).unwrap();
//         }
//         loro_b.import(loro.export(loro_b.vv_cloned()));
//         loro.import(loro_b.export(loro.vv_cloned()));
//     }
//     let encoded = loro.encode_with_cfg(EncodeMode::RleUpdates(VersionVector::new()));
//     println!("parallel doc size {} bytes", encoded.len());
//     let encoded = loro.encode_all();
//     println!("parallel doc size {} bytes", encoded.len());
// }

// #[allow(dead_code)]
// fn real_time() {
//     let actions = get_automerge_actions();
//     let mut c1 = LoroCore::new(Default::default(), Some(0));
//     let mut c2 = LoroCore::new(Default::default(), Some(1));
//     let mut t1 = c1.get_text("text");
//     let mut t2 = c2.get_text("text");
//     for (i, action) in actions.iter().enumerate() {
//         if i > 2000 {
//             break;
//         }
//         let TextAction { pos, ins, del } = action;
//         if i % 2 == 0 {
//             t1.delete(&c1, *pos, *del).unwrap();
//             t1.insert(&c1, *pos, ins).unwrap();
//             let update = c1.encode_with_cfg(EncodeMode::Updates(c2.vv_cloned()));
//             c2.decode(&update).unwrap();
//         } else {
//             t2.delete(&c2, *pos, *del).unwrap();
//             t2.insert(&c2, *pos, ins).unwrap();
//             let update = c2.encode_with_cfg(EncodeMode::Updates(c1.vv_cloned()));
//             c1.decode(&update).unwrap();
//         }
//     }
// }

fn main() {
    let actions = get_automerge_actions();
    let loro = LoroDoc::default();
    let loro_b = LoroDoc::default();
    let text = loro.get_text("text");
    let mut count = 0;
    for TextAction { pos, ins, del } in actions.iter() {
        {
            let mut txn = loro.txn().unwrap();
            text.delete_with_txn(&mut txn, *pos, *del).unwrap();
            text.insert_with_txn(&mut txn, *pos, ins).unwrap();
        }

        loro_b
            .import(&loro.export_from(&loro_b.oplog_vv()))
            .unwrap();
        count += 1;
        if count % 1000 == 0 {
            println!("{count}");
        }
    }
}
