use bench_utils::{get_automerge_actions, TextAction};
use loro_internal::{log_store::EncodeConfig, LoroCore, VersionVector};

fn main() {
    let actions = get_automerge_actions();
    let mut loro = LoroCore::default();
    let mut loro_b = LoroCore::default();
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
        text.delete(&loro_b, pos, del).unwrap();
        text.insert(&loro_b, pos, ins).unwrap();
        loro_b.import(loro.export(loro_b.vv_cloned()));
        loro.import(loro_b.export(loro.vv_cloned()));
    }
    let encoded = loro.encode_with_cfg(EncodeConfig::rle_update(VersionVector::new()));
    println!("parallel doc size {} bytes", encoded.len());
}
