use bench_utils::draw::{gen_draw_actions, DrawAction};
use criterion::{criterion_group, criterion_main, Criterion};
use loro_internal::LoroDoc;

pub fn draw(c: &mut Criterion) {
    let mut data = None;
    c.bench_function("simulate drawing", |b| {
        if data.is_none() {
            data = Some(gen_draw_actions(100, 1000));
        }

        let mut loro = LoroDoc::new();
        b.iter(|| {
            loro = LoroDoc::new();
            let paths = loro.get_list("all_paths");
            let texts = loro.get_list("all_texts");
            for action in data.as_ref().unwrap().iter() {
                match action {
                    DrawAction::DrawPath { points, color } => {}
                    DrawAction::Text {
                        id,
                        text,
                        pos,
                        width,
                        height,
                    } => todo!(),
                }
            }
        });

        println!("Snapshot size = {}", loro.export_snapshot().len())
    });
}

criterion_group!(benches, draw);
criterion_main!(benches);
