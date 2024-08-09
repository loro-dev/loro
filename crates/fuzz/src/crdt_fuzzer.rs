use std::{
    fmt::{Debug, Display},
    time::Instant,
};

use arbitrary::Arbitrary;
use fxhash::FxHashSet;
use loro::{ContainerType, Frontiers};
use tabled::TableIteratorExt;
use tracing::{info, info_span};

use crate::{actions::ActionWrapper, array_mut_ref};

pub use super::actions::Action;
use super::actor::Actor;

#[derive(Arbitrary, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzValue {
    I32(i32),
    Container(ContainerType),
}

impl Display for FuzzValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FuzzValue::I32(i) => write!(f, "{}", i),
            FuzzValue::Container(c) => write!(f, "{}", c),
        }
    }
}

struct CRDTFuzzer {
    actors: Vec<Actor>,
    targets: FxHashSet<ContainerType>,
}

impl CRDTFuzzer {
    fn new(site_num: u8, fuzz_targets: Vec<FuzzTarget>) -> Self {
        let mut actors = Vec::new();
        for i in 0..site_num {
            actors.push(Actor::new(i as u64));
        }

        let targets = fuzz_targets
            .into_iter()
            .map(|t| t.support_container_type())
            .fold(FxHashSet::default(), |mut acc, set| {
                acc.extend(set);
                acc
            });

        for target in targets.iter() {
            for actor in actors.iter_mut() {
                actor.register(*target);
            }
        }
        Self { actors, targets }
    }

    fn pre_process(&mut self, action: &mut Action) {
        let max_users = self.site_num() as u8;
        match action {
            Action::Sync { from, to } => {
                *from %= max_users;
                *to %= max_users;
                if to == from {
                    *to = (*to + 1) % max_users;
                }
            }
            Action::SyncAll => {}
            Action::Checkout { site, to } => {
                *site %= max_users;
                *to %= self.actors[*site as usize].history.len() as u32;
            }
            Action::Handle {
                site,
                target,
                container,
                action,
            } => {
                if matches!(action, ActionWrapper::Action(_)) {
                    return;
                }
                *site %= max_users;
                let actor = &mut self.actors[*site as usize];
                *target %= self.targets.len() as u8;
                let target = self.targets.iter().nth(*target as usize).unwrap();
                action.convert_to_inner(target);
                actor.pre_process(action.as_action_mut().unwrap(), container);
            }
            Action::Undo { site, op_len } => {
                *site %= max_users;
                let actor = &mut self.actors[*site as usize];
                *op_len %= actor.undo_manager.can_undo_length as u32 + 1;
            }
            Action::SyncAllUndo { site, op_len } => {
                *site %= max_users;
                let actor = &mut self.actors[*site as usize];
                *op_len %= actor.undo_manager.can_undo_length as u32 + 1;
            }
        }
    }

    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::SyncAll => {
                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to 0 from {}", i).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        a.loro
                            .import(&b.loro.export_from(&a.loro.oplog_vv()))
                            .unwrap();
                    });
                }

                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to {} from {}", i, 0).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        b.loro
                            .import(&a.loro.export_from(&b.loro.oplog_vv()))
                            .unwrap();
                    });
                }
                self.actors.iter_mut().for_each(|a| a.record_history());
                // for i in 0..self.site_num() {
                //     self.actors[i].loro.check_state_correctness_slow();
                // }
            }
            Action::Sync { from, to } => {
                let (a, b) = array_mut_ref!(&mut self.actors, [*from as usize, *to as usize]);
                a.loro
                    .import(&b.loro.export_from(&a.loro.oplog_vv()))
                    .unwrap();
                b.loro
                    .import(&a.loro.export_from(&b.loro.oplog_vv()))
                    .unwrap();
                a.record_history();
                b.record_history();
            }
            Action::Checkout { site, to } => {
                let actor = &mut self.actors[*site as usize];
                let f = actor.history.keys().nth(*to as usize).unwrap();
                let f = Frontiers::from(f);
                actor.loro.checkout(&f).unwrap();
            }
            Action::Handle {
                site,
                target: _,
                container,
                action,
            } => {
                let actor = &mut self.actors[*site as usize];
                let action = action.as_action().unwrap();
                actor.apply(action, *container);
            }
            Action::Undo { site, op_len } => {
                let actor = &mut self.actors[*site as usize];
                if *op_len != 0 {
                    actor.undo(*op_len);
                }
            }
            Action::SyncAllUndo { site, op_len } => {
                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to 0 from {}", i).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        a.loro
                            .import(&b.loro.export_from(&a.loro.oplog_vv()))
                            .unwrap();
                    });
                }

                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to {} from {}", i, 0).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        b.loro
                            .import(&a.loro.export_from(&b.loro.oplog_vv()))
                            .unwrap();
                    });
                }
                self.actors.iter_mut().for_each(|a| a.record_history());
                let actor = &mut self.actors[*site as usize];
                if *op_len != 0 {
                    actor.undo(*op_len);
                }
            }
        }
    }

    fn check_equal(&mut self) {
        for i in 0..self.site_num() - 1 {
            for j in i + 1..self.site_num() {
                let _s = info_span!("checking eq", ?i, ?j);
                let _g = _s.enter();
                let (a, b) = array_mut_ref!(&mut self.actors, [i, j]);
                let a_doc = &mut a.loro;
                let b_doc = &mut b.loro;
                info_span!("Attach", peer = i).in_scope(|| {
                    a_doc.attach();
                });
                info_span!("Attach", peer = j).in_scope(|| {
                    b_doc.attach();
                });
                match (i + j) % 3 {
                    0 => {
                        info_span!("Updates", from = j, to = i).in_scope(|| {
                            a_doc.import(&b_doc.export_from(&a_doc.oplog_vv())).unwrap();
                        });
                        info_span!("Updates", from = i, to = j).in_scope(|| {
                            b_doc.import(&a_doc.export_from(&b_doc.oplog_vv())).unwrap();
                        });
                    }
                    1 => {
                        info_span!("Snapshot", from = i, to = j).in_scope(|| {
                            b_doc.import(&a_doc.export_snapshot()).unwrap();
                        });
                        info_span!("Snapshot", from = j, to = i).in_scope(|| {
                            a_doc.import(&b_doc.export_snapshot()).unwrap();
                        });
                    }
                    _ => {
                        info_span!("JsonFormat", from = i, to = j).in_scope(|| {
                            let a_json =
                                a_doc.export_json_updates(&b_doc.oplog_vv(), &a_doc.oplog_vv());
                            b_doc.import_json_updates(a_json).unwrap();
                        });
                        info_span!("JsonFormat", from = j, to = i).in_scope(|| {
                            let b_json =
                                b_doc.export_json_updates(&a_doc.oplog_vv(), &b_doc.oplog_vv());
                            a_doc.import_json_updates(b_json).unwrap();
                        });
                    }
                }
                a.check_eq(b);
                a.record_history();
                b.record_history();
            }
        }
    }

    fn check_tracker(&self) {
        for actor in self.actors.iter() {
            actor.check_tracker();
        }
    }

    fn check_history(&mut self) {
        self.actors[0].check_history();
        // for actor in self.actors.iter_mut() {
        //     actor.check_history();
        // }
    }

    fn site_num(&self) -> usize {
        self.actors.len()
    }
}

#[derive(Eq, Hash, PartialEq)]
pub enum FuzzTarget {
    Map,
    List,
    Text,
    Tree,
    MovableList,
    Counter,
    All,
}

impl FuzzTarget {
    pub(super) fn support_container_type(&self) -> FxHashSet<ContainerType> {
        let mut set = FxHashSet::default();
        match self {
            FuzzTarget::All => {
                set.insert(ContainerType::Map);
                set.insert(ContainerType::List);
                set.insert(ContainerType::Text);
                set.insert(ContainerType::Tree);
                set.insert(ContainerType::MovableList);
            }
            FuzzTarget::Map => {
                set.insert(ContainerType::Map);
            }
            FuzzTarget::List => {
                set.insert(ContainerType::List);
            }
            FuzzTarget::Text => {
                set.insert(ContainerType::Text);
            }
            FuzzTarget::Tree => {
                set.insert(ContainerType::Tree);
                set.insert(ContainerType::Map);
            }
            FuzzTarget::MovableList => {
                set.insert(ContainerType::MovableList);
            }
            FuzzTarget::Counter => {
                set.insert(ContainerType::Counter);
            }
        }
        set
    }
}

pub fn test_multi_sites(site_num: u8, fuzz_targets: Vec<FuzzTarget>, actions: &mut [Action]) {
    let mut fuzzer = CRDTFuzzer::new(site_num, fuzz_targets);
    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        // info!("{:?}", action);
        fuzzer.pre_process(action);
        // info!("#####");
        info_span!("ApplyAction", ?action).in_scope(|| {
            applied.push(action.clone());
            info!("OptionsTable \n{}", (&applied).table());
            // info!("Apply Action {:?}", applied);
            fuzzer.apply_action(action);
        });
    }

    let span = &info_span!("check synced");
    let _g = span.enter();
    fuzzer.check_equal();
    fuzzer.check_tracker();
    fuzzer.check_history();
}

pub fn minify_error<T, F, N>(site_num: u8, f: F, normalize: N, actions: Vec<T>)
where
    F: Fn(u8, &mut [T]),
    N: Fn(u8, &mut [T]) -> Vec<T>,
    T: Clone + Debug,
{
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
        // println!("{:?}", _info);
    }));

    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
    #[allow(clippy::redundant_clone)]
    let mut actions_clone = actions.clone();
    let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
    #[allow(clippy::blocks_in_conditions)]
    if std::panic::catch_unwind(|| {
        // SAFETY: test
        let f = unsafe { &*(f_ref as *const F) };
        // SAFETY: test
        let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
        f(site_num, actions_ref);
    })
    .is_ok()
    {
        println!("No Error Found");
        return;
    }

    let mut minified = actions.clone();
    let mut candidates = Vec::new();
    println!("Setup candidates...");
    for i in 0..actions.len() {
        let mut new = actions.clone();
        new.remove(i);
        candidates.push(new);
    }

    println!("Minifying...");
    let start = Instant::now();
    while let Some(candidate) = candidates.pop() {
        let f_ref: *const _ = &f;
        let f_ref: usize = f_ref as usize;
        let mut actions_clone = candidate.clone();
        let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
        #[allow(clippy::blocks_in_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: test
            let f = unsafe { &*(f_ref as *const F) };
            // SAFETY: test
            let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
            f(site_num, actions_ref);
        })
        .is_err()
        {
            for i in 0..candidate.len() {
                let mut new = candidate.clone();
                new.remove(i);
                candidates.push(new);
            }
            if candidate.len() < minified.len() {
                minified = candidate;
                println!("New min len={}", minified.len());
            }
            if candidates.len() > 40 {
                candidates.drain(0..30);
            }
        }
        if start.elapsed().as_secs() > 10 && minified.len() <= 4 {
            break;
        }
        if start.elapsed().as_secs() > 60 {
            break;
        }
    }

    let minified = normalize(site_num, &mut minified);
    println!(
        "Old Length {}, New Length {}",
        actions.len(),
        minified.len()
    );
    dbg!(&minified);
    if actions.len() > minified.len() {
        minify_error(site_num, f, normalize, minified);
    }
}

pub fn minify_simple<T, F, N>(site_num: u8, f: F, normalize: N, actions: Vec<T>)
where
    F: Fn(u8, &mut [T]),
    N: Fn(u8, &mut [T]) -> Vec<T>,
    T: Clone + Debug,
{
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
        // println!("{:?}", _info);
    }));
    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
    #[allow(clippy::redundant_clone)]
    let mut actions_clone = actions.clone();
    let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
    #[allow(clippy::blocks_in_conditions)]
    if std::panic::catch_unwind(|| {
        // SAFETY: test
        let f = unsafe { &*(f_ref as *const F) };
        // SAFETY: test
        let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
        f(site_num, actions_ref);
    })
    .is_ok()
    {
        println!("No Error Found");
        return;
    }
    let mut minified = actions.clone();
    let mut current_index = minified.len() as i64 - 1;
    while current_index >= 0 {
        let a = minified.remove(current_index as usize);
        let f_ref: *const _ = &f;
        let f_ref: usize = f_ref as usize;
        let mut actions_clone = minified.clone();
        let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
        let mut re = false;
        #[allow(clippy::blocks_in_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: test
            let f = unsafe { &*(f_ref as *const F) };
            // SAFETY: test
            let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
            f(site_num, actions_ref);
        })
        .is_err()
        {
            re = true;
        } else {
            minified.insert(current_index as usize, a);
        }
        println!(
            "{}/{} {}",
            actions.len() as i64 - current_index,
            actions.len(),
            re
        );
        current_index -= 1;
    }
    let minified = normalize(site_num, &mut minified);

    println!("{:?}", &minified);
    println!(
        "Old Length {}, New Length {}",
        actions.len(),
        minified.len()
    );
    if actions.len() > minified.len() {
        minify_simple(site_num, f, normalize, minified);
    }
}
