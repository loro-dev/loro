use loro_core::{LoroCore, LoroValue};

fn main() {
    let mut actors: Vec<_> = (0..500).map(|_| LoroCore::default()).collect();
    for (i, actor) in actors.iter_mut().enumerate() {
        let mut map = actor.get_map("map");
        let value: LoroValue = i.to_string().into();
        map.insert(actor, &i.to_string(), value).unwrap();
    }

    for i in 1..actors.len() {
        let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
        a.import(b.export(a.vv()));
    }

    for i in 1..actors.len() {
        let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
        b.import(a.export(b.vv()));
    }
}
