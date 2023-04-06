use bench_utils::TextAction;
use loro_internal::log_store::EncodeMode;
use loro_internal::{LoroCore, Transact, VersionVector};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

fn main() {
    let mut rng: StdRng = SeedableRng::seed_from_u64(1);
    let actions = bench_utils::get_automerge_actions();
    let mut updates = Vec::new();
    let mut last_vv = VersionVector::new();
    let mut loro = LoroCore::new(Default::default(), Some(1));
    let mut text = loro.get_text("text");
    let mut action_iter = actions.into_iter().take(100000).peekable();

    while action_iter.peek_mut().is_some() {
        let n = rng.gen_range(20..80);
        let txn = loro.transact();
        for _ in 0..n {
            let Some(TextAction { pos, ins, del }) = action_iter.next()else{break;};
            text.delete(&txn, pos, del).unwrap();
            text.insert(&txn, pos, ins).unwrap();
        }
        drop(txn);
        let mode = match rng.gen_range(0..=10) {
            0 => "snapshot",
            1..=5 => "updates",
            _ => "changes",
        };
        let overlap = rng.gen_range(0..=(*last_vv.get(&1).unwrap_or(&0)).min(10));
        *last_vv.get_mut(&1).unwrap_or(&mut 0) -= overlap;
        let update = match mode {
            "snapshot" => loro.encode_all(),
            "updates" => loro.encode_with_cfg(EncodeMode::Updates(last_vv.clone())),
            "changes" => loro.encode_with_cfg(EncodeMode::RleUpdates(last_vv.clone())),
            _ => unreachable!(),
        };
        updates.push(update);
        last_vv = loro.vv_cloned();
    }
    updates.shuffle(&mut rng);
    for _ in 0..1 {
        let mut loro2 = LoroCore::default();
        // loro2.decode_batch(&updates).unwrap();
        for (i, u) in updates.iter().enumerate() {
            loro2.decode(u).unwrap();
        }
    }
}
