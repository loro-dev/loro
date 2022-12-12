use loro_core::{LoroCore, LoroValue};
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let mut actors: Vec<_> = (0..200).map(|_| LoroCore::default()).collect();
    for (i, actor) in actors.iter_mut().enumerate() {
        let mut list = actor.get_list("list");
        let value: LoroValue = i.to_string().into();
        list.insert(actor, 0, value).unwrap();
    }

    for i in 1..actors.len() {
        let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
        a.import(b.export(a.vv()));
    }

    for i in 1..actors.len() {
        let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
        b.import(a.export(b.vv()));
    }
    drop(profiler);
}
