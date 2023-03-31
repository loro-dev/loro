fn main() {
    use bench_utils::TextAction;
    use loro_internal::LoroCore;
    use std::time::Instant;

    let actions = bench_utils::get_automerge_actions();
    let mut loro = LoroCore::default();
    let start = Instant::now();
    // loro.subscribe_deep(Box::new(|_| ()));
    let mut text = loro.get_text("text");
    for _ in 0..1 {
        for TextAction { del, ins, pos } in actions.iter() {
            text.delete_utf16(&loro, *pos, *del).unwrap();
            text.insert_utf16(&loro, *pos, ins).unwrap();
        }
    }
    loro.diagnose();
    println!("{}", start.elapsed().as_millis());
}
