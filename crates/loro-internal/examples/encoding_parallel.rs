use bench_utils::{get_automerge_actions, TextAction};
use loro_internal::{log_store::EncodeConfig, LoroCore, VersionVector};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn main() {
    let mut rng: StdRng = SeedableRng::seed_from_u64(1);

    let actions = get_automerge_actions();
    let mut loro = LoroCore::new(Default::default(), Some(1));
    let mut loro_b = LoroCore::new(Default::default(), Some(2));
    let mut i = 0;
    for TextAction { pos, ins, del } in actions.iter() {
        i += 1;
        if i > 1000 {
            break;
        }
        let pos = *pos;
        let del = *del;
        let mut text = loro.get_text("text");
        text.delete(&loro, pos, del).unwrap();
        text.insert(&loro, pos, ins).unwrap();
        let mut text = loro_b.get_text("text");
        let r = rng.gen_range(1..11);
        for _ in 0..r {
            text.delete(&loro_b, pos, del).unwrap();
            text.insert(&loro_b, pos, ins).unwrap();
        }
        loro_b.import(loro.export(loro_b.vv_cloned()));
        loro.import(loro_b.export(loro.vv_cloned()));
    }
    let encoded =
        loro.encode_with_cfg(EncodeConfig::rle_update(VersionVector::new()).without_compress());
    println!("parallel doc size {} bytes", encoded.len());
    let encoded = loro.encode_with_cfg(EncodeConfig::snapshot().without_compress());
    println!("parallel doc size {} bytes", encoded.len());
}
