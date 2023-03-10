#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use std::time::Instant;

use bench_utils::TextAction;
use loro_internal::LoroCore;

fn apply_automerge(times: usize) {
    let actions = bench_utils::get_automerge_actions();
    let start = Instant::now();
    let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let mut loro = LoroCore::default();
    let mut text = loro.get_text("text");
    println!("Apply Automerge Dataset 1X");
    for _i in 0..times {
        for TextAction { pos, ins, del } in actions.iter() {
            text.delete(&loro, *pos, *del).unwrap();
            text.insert(&loro, *pos, ins).unwrap();
        }
    }
    drop(profiler);
    println!("Used: {} ms", start.elapsed().as_millis());
}

fn concurrent_actors(actor_num: usize) {
    let mut actors: Vec<LoroCore> = Vec::new();
    for _ in 0..actor_num {
        actors.push(LoroCore::default());
    }

    let mut updates = Vec::new();
    for actor in actors.iter_mut() {
        let mut list = actor.get_list("list");
        list.insert(actor, 0, 1).unwrap();
        updates.push(actor.encode_all());
    }

    let mut a = actors.drain(0..1).next().unwrap();
    drop(actors);
    let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    for update in updates {
        a.decode(&update).unwrap();
    }
    drop(profiler);
}

fn realtime_sync(actor_num: usize, action_num: usize) {
    let actions = bench_utils::gen_realtime_actions(action_num, actor_num, 100);
    let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let mut actors = Vec::new();
    for _ in 0..actor_num {
        actors.push(LoroCore::default());
    }

    for action in actions {
        match action {
            bench_utils::Action::Text { client, action } => {
                let mut text = actors[client].get_text("text");
                let bench_utils::TextAction { pos, ins, del } = action;
                let pos = pos % (text.len() + 1);
                let del = del.min(text.len() - pos);
                text.delete(&actors[client], pos, del).unwrap();
                text.insert(&actors[client], pos, &ins).unwrap();
            }
            bench_utils::Action::SyncAll => {
                let mut updates = Vec::new();
                for i in 1..actor_num {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    updates.push(b.encode_from(a.vv_cloned()));
                }
                for update in updates {
                    // TODO: use import batch here
                    actors[0].decode(&update).unwrap();
                }
                for i in 1..actor_num {
                    let (a, b) = arref::array_mut_ref!(&mut actors, [0, i]);
                    b.decode(&a.encode_from(b.vv_cloned())).unwrap();
                }
            }
        }
    }
    drop(profiler);
}

pub fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 2 {
        apply_automerge(1);
        return;
    }

    match args[1].as_str() {
        "automerge" => {
            apply_automerge(1);
        }
        "100_concurrent" => {
            concurrent_actors(100);
        }
        "200_concurrent" => {
            concurrent_actors(200);
        }
        "10_actor_sync_1000_actions" => realtime_sync(10, 1000),
        "20_actor_sync_1000_actions" => realtime_sync(20, 1000),
        "10_actor_sync_2000_actions" => realtime_sync(10, 2000),
        _ => {
            panic!("Unknown command `{}`", args.join(" "));
        }
    }
}
