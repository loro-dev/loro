use std::time::Instant;

use bench_utils::{
    create_seed, gen_async_actions, gen_realtime_actions, make_actions_async, Action, ActionTrait,
};

pub mod draw;
pub mod json;
pub mod sheet;

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
            Action::Sync { from, to } => {
                let vv = self.docs[*to].doc().oplog_vv();
                let data = self.docs[*from].doc().export_from(&vv);
                self.docs[*to].doc().import(&data).unwrap();
            }
            Action::SyncAll => self.sync_all(),
        }
    }

    pub fn sync_all(&mut self) {
        debug_log::group!("SyncAll");
        let (first, rest) = self.docs.split_at_mut(1);
        for doc in rest.iter_mut() {
            let vv = first[0].doc().oplog_vv();
            first[0].doc().import(&doc.doc().export_from(&vv)).unwrap();
        }
        for doc in rest.iter_mut() {
            let vv = doc.doc().oplog_vv();
            doc.doc().import(&first[0].doc().export_from(&vv)).unwrap();
        }
        debug_log::group_end!();
    }

    pub fn check_sync(&self) {
        debug_log::group!("Check sync");
        let first = &self.docs[0];
        let content = first.doc().get_deep_value();
        for doc in self.docs.iter().skip(1) {
            assert_eq!(content, doc.doc().get_deep_value());
        }
        debug_log::group_end!();
    }
}

pub fn run_async_workflow<T: ActorTrait>(
    peer_num: usize,
    action_num: usize,
    actions_before_sync: usize,
    seed: u64,
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
        |_| {},
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
) -> (ActorGroup<T>, Instant)
where
    for<'a> T::ActionKind: arbitrary::Arbitrary<'a>,
{
    let seed = create_seed(seed, action_num * 32);
    let mut actions =
        gen_realtime_actions::<T::ActionKind>(action_num, peer_num, &seed, |_| {}).unwrap();
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
        debug_log::debug_log!("[ApplyAction] {:#?}", &action);
        actors.apply_action(action);
    }
    actors.sync_all();
    actors.check_sync();
}
