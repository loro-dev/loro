#![allow(deprecated)]
#![allow(clippy::uninlined_format_args)]
use bench_utils::{
    create_seed, gen_async_actions, gen_realtime_actions, make_actions_async, Action, ActionTrait,
};
use loro::ExportMode;
use std::{
    collections::VecDeque,
    sync::{atomic::AtomicUsize, Arc, Mutex},
    time::Instant,
};
use tracing::span;

pub mod draw;
pub mod json;
pub mod list;
pub mod sheet;
pub mod utils;
pub mod test_preload {
    pub use bench_utils::json::JsonAction::*;
    pub use bench_utils::json::LoroValue::*;
    pub use bench_utils::Action::*;
    pub use bench_utils::SyncKind::*;
}

pub trait ActorTrait {
    type ActionKind: ActionTrait;
    fn create(peer_id: u64) -> Self;
    fn apply_action(&mut self, action: &mut Self::ActionKind);
    fn doc(&self) -> &loro::LoroDoc;
}

pub struct ActorGroup<T> {
    pub docs: Vec<T>,
}

impl<T: ActorTrait> ActorGroup<T> {
    pub fn new(size: usize) -> Self {
        let docs = (0..size).map(|i| T::create(i as u64)).collect();
        Self { docs }
    }

    pub fn apply_action(&mut self, action: &mut Action<T::ActionKind>) {
        match action {
            Action::Action { peer, action } => {
                self.docs[*peer].apply_action(action);
            }
            Action::Sync { from, to, kind } => match kind {
                bench_utils::SyncKind::Fit => {
                    let vv = self.docs[*to].doc().oplog_vv();
                    let data = self.docs[*from]
                        .doc()
                        .export(ExportMode::updates(&vv))
                        .unwrap();
                    self.docs[*to].doc().import(&data).unwrap();
                }
                bench_utils::SyncKind::Snapshot => {
                    let data = self.docs[*from]
                        .doc()
                        .export(ExportMode::snapshot())
                        .unwrap();
                    self.docs[*to].doc().import(&data).unwrap();
                }
                bench_utils::SyncKind::OnlyLastOpFromEachPeer => {
                    let mut vv = self.docs[*from].doc().oplog_vv();
                    for cnt in vv.values_mut() {
                        *cnt -= 1;
                    }
                    let data = self.docs[*from]
                        .doc()
                        .export(ExportMode::updates(&vv))
                        .unwrap();
                    self.docs[*to].doc().import(&data).unwrap();
                }
            },
            Action::SyncAll => self.sync_all(),
        }
    }

    pub fn sync_all(&mut self) {
        let s = span!(tracing::Level::INFO, "SyncAll");
        let _enter = s.enter();
        let (first, rest) = self.docs.split_at_mut(1);
        for doc in rest.iter_mut() {
            let s = tracing::span!(tracing::Level::INFO, "Importing to doc0");
            let _e = s.enter();
            let vv = first[0].doc().oplog_vv();
            first[0]
                .doc()
                .import(&doc.doc().export(ExportMode::updates(&vv)).unwrap())
                .unwrap();
        }
        for (i, doc) in rest.iter_mut().enumerate() {
            let s = tracing::span!(tracing::Level::INFO, "Importing to doc", doc = i + 1);
            let _e = s.enter();
            let vv = doc.doc().oplog_vv();
            doc.doc()
                .import(&first[0].doc().export(ExportMode::updates(&vv)).unwrap())
                .unwrap();
        }
    }

    pub fn check_sync(&self) {
        let s = tracing::span!(tracing::Level::INFO, "Check sync");
        let _e = s.enter();
        let first = &self.docs[0];
        let content = first.doc().get_deep_value();
        for doc in self.docs.iter().skip(1) {
            assert_eq!(content, doc.doc().get_deep_value());
        }
    }
}

pub fn run_async_workflow<T: ActorTrait>(
    peer_num: usize,
    action_num: usize,
    actions_before_sync: usize,
    seed: u64,
    preprocess: impl FnMut(&mut Action<T::ActionKind>),
) -> (ActorGroup<T>, Instant)
where
    for<'a> T::ActionKind: arbitrary::Arbitrary<'a>,
{
    let seed = create_seed(seed, action_num * 32);
    let mut actions = gen_async_actions::<T::ActionKind>(
        action_num,
        peer_num,
        &seed,
        actions_before_sync,
        preprocess,
    )
    .unwrap();
    let mut actors = ActorGroup::<T>::new(peer_num);
    let start = Instant::now();
    for action in actions.iter_mut() {
        actors.apply_action(action);
    }

    (actors, start)
}

pub fn run_realtime_collab_workflow<T: ActorTrait>(
    peer_num: usize,
    action_num: usize,
    seed: u64,
    preprocess: impl FnMut(&mut Action<T::ActionKind>),
) -> (ActorGroup<T>, Instant)
where
    for<'a> T::ActionKind: arbitrary::Arbitrary<'a>,
{
    let seed = create_seed(seed, action_num * 32);
    let mut actions =
        gen_realtime_actions::<T::ActionKind>(action_num, peer_num, &seed, preprocess).unwrap();
    let mut actors = ActorGroup::<T>::new(peer_num);
    let start = Instant::now();
    for action in actions.iter_mut() {
        actors.apply_action(action);
    }

    (actors, start)
}

pub fn run_actions_fuzz_in_async_mode<T: ActorTrait>(
    peer_num: usize,
    sync_all_interval: usize,
    actions: &[Action<T::ActionKind>],
) {
    let mut actions = make_actions_async::<T::ActionKind>(peer_num, actions, sync_all_interval);
    let mut actors = ActorGroup::<T>::new(peer_num);
    for action in actions.iter_mut() {
        let s = tracing::span!(tracing::Level::INFO, "ApplyAction", ?action);
        let _e = s.enter();
        actors.apply_action(action);
    }
    actors.sync_all();
    actors.check_sync();
}

pub fn minify_failed_tests_in_async_mode<T: ActorTrait>(
    peer_num: usize,
    sync_all_interval: usize,
    actions: &[Action<T::ActionKind>],
) {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
        // println!("{:?}", _info);
    }));

    let actions = make_actions_async::<T::ActionKind>(peer_num, actions, sync_all_interval);
    let mut stack: VecDeque<Vec<Action<T::ActionKind>>> = VecDeque::new();
    stack.push_back(actions);
    let mut last_log = Instant::now();
    let mut min_actions: Option<Vec<Action<T::ActionKind>>> = None;
    while let Some(actions) = stack.pop_back() {
        let actions = Arc::new(Mutex::new(actions));
        let actions_clone = Arc::clone(&actions);
        let num = Arc::new(AtomicUsize::new(0));
        let num_clone = Arc::clone(&num);
        let result = std::panic::catch_unwind(move || {
            let mut actors = ActorGroup::<T>::new(peer_num);
            for action in actions_clone.lock().unwrap().iter_mut() {
                actors.apply_action(action);
                num_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            actors.sync_all();
            actors.check_sync();
        });

        if result.is_ok() {
            continue;
        }

        let num = num.load(std::sync::atomic::Ordering::SeqCst);
        let mut actions = match actions.lock() {
            Ok(a) => a,
            Err(a) => a.into_inner(),
        };
        actions.drain(num..);
        if let Some(min_actions) = min_actions.as_mut() {
            if actions.len() < min_actions.len() {
                min_actions.clone_from(&actions);
            }
        } else {
            min_actions = Some(actions.clone());
        }

        for i in 0..actions.len() {
            let mut new_actions = actions.clone();
            new_actions.remove(i);
            stack.push_back(new_actions);
        }

        while stack.len() > 100 {
            stack.pop_front();
        }

        if last_log.elapsed().as_secs() > 1 {
            println!(
                "stack size: {}. Min action size {:?}",
                stack.len(),
                min_actions.as_ref().map(|x| x.len())
            );
            last_log = Instant::now();
        }
    }

    if let Some(minimal_failed_actions) = min_actions {
        println!("Min action size {:?}", minimal_failed_actions.len());
        println!("{minimal_failed_actions:#?}");
        std::panic::set_hook(hook);
        run_actions_fuzz_in_async_mode::<T>(peer_num, sync_all_interval, &minimal_failed_actions);
    } else {
        println!("No failed tests found");
    }
}
