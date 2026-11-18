use loro::{LoroDoc, LoroValue, ValueOrContainer};
use std::{hint::black_box, time::Instant};

fn main() {
    let scenario = std::env::args().nth(1).unwrap();
    let length: usize = std::env::args().nth(2).unwrap().parse().unwrap();
    if scenario == "delta" {
        let mut combined = loro_internal::event::TextDiff::new();
        let started = Instant::now();
        for position in (0..length).rev() {
            let mut next = loro_internal::event::TextDiff::new();
            next.push_retain(position, Default::default());
            next.push_delete(1);
            combined.compose(&next);
        }
        let elapsed_ns = started.elapsed().as_nanos();
        println!("{scenario},{length},{elapsed_ns},{}", combined.iter().count());
        return;
    }
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_list("list");
    let movable = doc.get_movable_list("movable");
    for index in 0..length {
        list.push(index as i64).unwrap();
        movable.push(index as i64).unwrap();
    }
    doc.commit();
    let mut visits = 0usize;
    let started = Instant::now();
    for _ in 0..100_000 {
        let mut consume = |value: ValueOrContainer| {
            assert!(matches!(black_box(value), ValueOrContainer::Value(LoroValue::I64(_))));
            visits += 1;
        };
        match scenario.as_str() {
            "list" => list.for_each(&mut consume),
            "movable" => movable.for_each(&mut consume),
            _ => panic!("unknown scenario"),
        }
    }
    let elapsed_ns = started.elapsed().as_nanos();
    assert_eq!(visits, length * 100_000);
    println!("{scenario},{length},{elapsed_ns}");
}
