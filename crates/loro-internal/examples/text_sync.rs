#[cfg(not(feature = "test_utils"))]
fn main() {}

#[cfg(feature = "test_utils")]
fn main() {
    use std::time::Instant;

    use bench_utils::{get_automerge_actions, TextAction};
    use loro_internal::LoroCore;

    let actions = get_automerge_actions();
    let mut loro = LoroCore::default();
    let mut loro_b = LoroCore::default();
    let mut loro_c = LoroCore::default();
    let start = Instant::now();
    for (i, TextAction { pos, ins, del }) in actions.iter().enumerate() {
        let mut text = loro.get_text("text");
        text.delete(&loro, *pos, *del).unwrap();
        text.insert(&loro, *pos, ins).unwrap();

        let mut text = loro_b.get_text("text");
        text.delete(&loro_b, *pos, *del).unwrap();
        text.insert(&loro_b, *pos, ins).unwrap();
        if i % 10 == 0 {
            loro.import(loro_b.export(loro.vv_cloned()));
            loro_b.import(loro.export(loro_b.vv_cloned()));
        }
    }
    loro_b.debug_inspect();
    loro.debug_inspect();
    println!("Elapsed {}ms", start.elapsed().as_millis());
    loro_c.import(loro.export(loro_c.vv_cloned()));
    println!("Elapsed {}ms", start.elapsed().as_millis());
}
