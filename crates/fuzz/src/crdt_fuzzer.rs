use std::fmt::Debug;

use arbitrary::Arbitrary;
use fxhash::FxHashSet;
use loro::{ContainerType, Frontiers};
use tabled::TableIteratorExt;
use tracing::{info, info_span, trace};

use crate::array_mut_ref;

pub use super::actions::Action;
use super::actor::Actor;

#[derive(Arbitrary, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzValue {
    I32(i32),
    Container(ContainerType),
}

impl ToString for FuzzValue {
    fn to_string(&self) -> String {
        match self {
            FuzzValue::I32(i) => i.to_string(),
            FuzzValue::Container(c) => c.to_string(),
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
                *site %= max_users;
                let actor = &mut self.actors[*site as usize];
                *target %= self.targets.len() as u8;
                let target = self.targets.iter().nth(*target as usize).unwrap();
                action.convert_to_inner(target);
                actor.pre_process(action.as_action_mut().unwrap(), container);
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
                // actor.loro.commit();
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
                if (i + j) % 2 == 0 {
                    info_span!("Updates", from = j, to = i).in_scope(|| {
                        a_doc.import(&b_doc.export_from(&a_doc.oplog_vv())).unwrap();
                    });
                    info_span!("Updates", from = i, to = j).in_scope(|| {
                        b_doc.import(&a_doc.export_from(&b_doc.oplog_vv())).unwrap();
                    });
                } else {
                    info_span!("Snapshot", from = i, to = j).in_scope(|| {
                        b_doc.import(&a_doc.export_snapshot()).unwrap();
                    });
                    info_span!("Snapshot", from = j, to = i).in_scope(|| {
                        a_doc.import(&b_doc.export_snapshot()).unwrap();
                    });
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
        for actor in self.actors.iter_mut() {
            actor.check_history();
        }
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
        }
        set
    }
}

pub fn test_multi_sites(site_num: u8, fuzz_targets: Vec<FuzzTarget>, actions: &mut [Action]) {
    let mut fuzzer = CRDTFuzzer::new(site_num, fuzz_targets);

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        fuzzer.pre_process(action);
        info_span!("ApplyAction", ?action).in_scope(|| {
            applied.push(action.clone());
            info!("OptionsTable \n{}", (&applied).table());
            fuzzer.apply_action(action);
        });
    }
    let span = &info_span!("check synced");
    let _g = span.enter();
    fuzzer.check_equal();
    fuzzer.check_tracker();
    fuzzer.check_history();
}
