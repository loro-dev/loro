use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
    fs,
    io::{self, Write},
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{atomic::AtomicUsize, Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use arbitrary::Arbitrary;
use loro::{
    ContainerType, ExportMode, Frontiers, ImportStatus, LoroDoc, LoroError, LoroResult, TreeID,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rustc_hash::FxHashSet;
use tabled::TableIteratorExt;
use tracing::{info, info_span};

use crate::{
    actions::{ActionWrapper, GenericAction},
    array_mut_ref,
};

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
            Action::ForkAt { site, to } => {
                *site %= max_users;
                *to %= self.actors[*site as usize].history.len() as u32;
            }
            Action::DiffApply { from, to } => {
                *from %= max_users;
                *to %= max_users;
                if from == to {
                    *to = (*to + 1) % max_users;
                }
            }
            Action::Query { site, target, .. } => {
                *site %= max_users;
                *target %= self.targets.len() as u8;
            }
            Action::ExportShallow { site } => {
                *site %= max_users;
            }
            Action::ImportShallow { site, from } => {
                *site %= max_users;
                *from %= max_users;
                if site == from {
                    *from = (*from + 1) % max_users;
                }
            }
            Action::StateOnlyRoundTrip { site } => {
                *site %= max_users;
            }
            Action::Commit { site } => {
                *site %= max_users;
            }
            Action::SetCommitOptions { site, .. } => {
                *site %= max_users;
            }
        }
    }

    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::SyncAll => {
                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to 0 from {}", i).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        handle_import_result(
                            a.loro.import(
                                &b.loro
                                    .export(ExportMode::updates(&a.loro.oplog_vv()))
                                    .unwrap(),
                            ),
                        );
                    });
                }

                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to {} from {}", i, 0).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        handle_import_result(
                            b.loro.import(
                                &a.loro
                                    .export(ExportMode::updates(&b.loro.oplog_vv()))
                                    .unwrap(),
                            ),
                        )
                    });
                }
                self.actors.iter_mut().for_each(|a| a.record_history());
                // for i in 0..self.site_num() {
                //     self.actors[i].loro.check_state_correctness_slow();
                // }
            }
            Action::Sync { from, to } => {
                let (a, b) = array_mut_ref!(&mut self.actors, [*from as usize, *to as usize]);
                handle_import_result(
                    a.loro.import(
                        &b.loro
                            .export(ExportMode::updates(&a.loro.oplog_vv()))
                            .unwrap(),
                    ),
                );
                handle_import_result(
                    b.loro.import(
                        &a.loro
                            .export(ExportMode::updates(&b.loro.oplog_vv()))
                            .unwrap(),
                    ),
                );
                a.record_history();
                b.record_history();
            }
            Action::Checkout { site, to } => {
                let actor = &mut self.actors[*site as usize];
                let f = actor.history.keys().nth(*to as usize).unwrap();
                let f = Frontiers::from(f);
                match actor.loro.checkout(&f) {
                    Ok(_) => {}
                    Err(LoroError::SwitchToVersionBeforeShallowRoot) => {}
                    Err(e) => panic!("{}", e),
                }
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
                actor.loro.commit();
            }
            Action::Undo { site, op_len } => {
                let actor = &mut self.actors[*site as usize];
                let undo_len = *op_len % 16;
                if undo_len != 0 {
                    actor.test_undo(undo_len);
                }
            }
            Action::SyncAllUndo { site, op_len } => {
                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to 0 from {}", i).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        handle_import_result(
                            a.loro.import(
                                &b.loro
                                    .export(ExportMode::updates(&a.loro.oplog_vv()))
                                    .unwrap(),
                            ),
                        );
                    });
                }

                for i in 1..self.site_num() {
                    info_span!("Importing", "importing to {} from {}", i, 0).in_scope(|| {
                        let (a, b) = array_mut_ref!(&mut self.actors, [0, i]);
                        handle_import_result(
                            b.loro.import(
                                &a.loro
                                    .export(ExportMode::updates(&b.loro.oplog_vv()))
                                    .unwrap(),
                            ),
                        );
                    });
                }
                self.actors.iter_mut().for_each(|a| a.record_history());
                let actor = &mut self.actors[*site as usize];
                let undo_len = *op_len % 8;
                if undo_len != 0 {
                    actor.test_undo(undo_len);
                }
            }
            Action::ForkAt { site, to } => {
                let actor = &mut self.actors[*site as usize];
                let f = actor.history.keys().nth(*to as usize).unwrap();
                let f = Frontiers::from(f);
                let _forked = actor.loro.fork_at(&f);
            }
            Action::DiffApply { from, to } => {
                let (a, b) = array_mut_ref!(&mut self.actors, [*from as usize, *to as usize]);
                let a_frontiers = a.loro.oplog_frontiers();
                let b_frontiers = b.loro.oplog_frontiers();
                if let Ok(diff) = a.loro.diff(&a_frontiers, &b_frontiers) {
                    let before_apply = b.loro.state_frontiers();
                    let result = b.loro.apply_diff(diff);
                    if result.is_ok() || b.loro.state_frontiers() != before_apply {
                        b.loro.commit();
                    }
                }
            }
            Action::Query {
                site,
                target,
                query_type,
            } => {
                let actor = &self.actors[*site as usize];
                let targets: Vec<_> = actor.targets.keys().copied().collect();
                if targets.is_empty() {
                    return;
                }
                let ty = targets[*target as usize % targets.len()];
                match ty {
                    ContainerType::Text => {
                        let text = actor.loro.get_text("text");
                        match *query_type % 8 {
                            0 => {
                                let _ = text.to_delta();
                            }
                            1 => {
                                let _ = text.len_unicode();
                            }
                            2 => {
                                let _ = text.len_utf8();
                            }
                            3 => {
                                let len = text.len_unicode();
                                if len > 0 {
                                    let _ = text.get_cursor(len / 2, loro::cursor::Side::Left);
                                }
                            }
                            4 => {
                                let len = text.len_unicode();
                                if len > 0 {
                                    let _ = text.slice(0, len / 2);
                                }
                            }
                            5 => {
                                let len = text.len_utf8();
                                if len > 0 {
                                    let _ = text.slice(0, len / 2);
                                }
                            }
                            _ => {
                                let _ = text.to_string();
                            }
                        }
                    }
                    ContainerType::List => {
                        let list = actor.loro.get_list("list");
                        match *query_type % 4 {
                            0 => {
                                let _ = list.len();
                            }
                            1 => {
                                let _ = list.to_vec();
                            }
                            2 => {
                                let len = list.len();
                                if len > 0 {
                                    let _ = list.get(len / 2);
                                }
                            }
                            _ => {
                                let _ = list.is_empty();
                            }
                        }
                    }
                    ContainerType::Map => {
                        let map = actor.loro.get_map("map");
                        match *query_type % 4 {
                            0 => {
                                let _ = map.keys();
                            }
                            1 => {
                                let _ = map.values();
                            }
                            2 => {
                                let _ = map.len();
                            }
                            _ => {
                                let _ = map.is_empty();
                            }
                        }
                    }
                    ContainerType::Tree => {
                        let tree = actor.loro.get_tree("tree");
                        match *query_type % 8 {
                            0 => {
                                let _ = tree.nodes();
                            }
                            1 => {
                                let _ = tree.children(None);
                            }
                            2 => {
                                let _ = tree.children_num(None);
                            }
                            3 => {
                                let _ = tree.contains(TreeID::new(0, 0));
                            }
                            4 => {
                                let _ = tree.is_node_deleted(&TreeID::new(0, 0));
                            }
                            5 => {
                                let _ = tree.parent(TreeID::new(0, 0));
                            }
                            _ => {
                                let _ = tree.is_empty();
                            }
                        }
                    }
                    ContainerType::MovableList => {
                        let list = actor.loro.get_movable_list("movable_list");
                        match *query_type % 4 {
                            0 => {
                                let _ = list.len();
                            }
                            1 => {
                                let _ = list.to_vec();
                            }
                            2 => {
                                let len = list.len();
                                if len > 0 {
                                    let _ = list.get(len / 2);
                                }
                            }
                            _ => {
                                let _ = list.is_empty();
                            }
                        }
                    }
                    ContainerType::Counter => {
                        let counter = actor.loro.get_counter("counter");
                        let _ = counter.get();
                    }
                    ContainerType::Unknown(_) => {}
                }
            }
            Action::ExportShallow { site } => {
                let actor = &self.actors[*site as usize];
                let f = actor.loro.oplog_frontiers();
                if !f.is_empty() {
                    let _ = actor.loro.export(loro::ExportMode::shallow_snapshot(&f));
                }
            }
            Action::ImportShallow { site, from } => {
                let (a, b) = array_mut_ref!(&mut self.actors, [*site as usize, *from as usize]);
                let f = b.loro.oplog_frontiers();
                if !f.is_empty() {
                    if let Ok(bytes) = b.loro.export(loro::ExportMode::shallow_snapshot(&f)) {
                        let _ = a.import_with_tracking(&bytes);
                    }
                }
            }
            Action::StateOnlyRoundTrip { site } => {
                let actor = &mut self.actors[*site as usize];
                let f = actor.loro.state_frontiers();
                if !f.is_empty() {
                    if let Ok(bytes) = actor.loro.export(loro::ExportMode::state_only(Some(&f))) {
                        let new_doc = LoroDoc::new();
                        if new_doc.import(&bytes).is_ok() {
                            assert_eq!(
                                new_doc.get_deep_value(),
                                actor.loro.get_deep_value(),
                                "site={site} state_frontiers={:?} oplog_frontiers={:?} oplog_vv={:?} imported_frontiers={:?} imported_vv={:?} shallow_frontiers={:?} shallow_vv={:?}",
                                actor.loro.state_frontiers(),
                                actor.loro.oplog_frontiers(),
                                actor.loro.oplog_vv(),
                                new_doc.oplog_frontiers(),
                                new_doc.oplog_vv(),
                                new_doc.shallow_since_frontiers(),
                                new_doc.shallow_since_vv(),
                            );
                        }
                    }
                }
            }
            Action::Commit { site } => {
                self.actors[*site as usize].loro.commit();
            }
            Action::SetCommitOptions { site, origin, msg } => {
                let actor = &self.actors[*site as usize];
                let origins = ["fuzz", "test", "a", "b", "c", "d", "e", "f"];
                actor
                    .loro
                    .set_next_commit_origin(origins[*origin as usize % origins.len()]);
                let msgs = ["msg1", "msg2", "hello", "world"];
                actor
                    .loro
                    .set_next_commit_message(msgs[*msg as usize % msgs.len()]);
            }
        }
    }

    fn check_equal(&mut self) {
        for i in 0..self.site_num() - 1 {
            for j in i + 1..self.site_num() {
                let _s = info_span!("checking eq", ?i, ?j);
                let _g = _s.enter();
                let (a, b) = array_mut_ref!(&mut self.actors, [i, j]);
                let a_shallow = a.loro.is_shallow();
                let b_shallow = b.loro.is_shallow();
                // Shallow docs cannot export ops before the shallow root, so
                // they cannot sync complete history to empty peers. Skip sync
                // checks for pairs where either side is shallow.
                if a_shallow || b_shallow {
                    continue;
                }
                let a_doc = &a.loro;
                let b_doc = &b.loro;
                info_span!("Attach", peer = i).in_scope(|| {
                    a_doc.attach();
                });
                info_span!("Attach", peer = j).in_scope(|| {
                    b_doc.attach();
                });
                match (i + j) % 4 {
                    0 => {
                        info_span!("Updates", from = j, to = i).in_scope(|| {
                            a_doc
                                .import(
                                    &b_doc
                                        .export(ExportMode::updates(&a_doc.oplog_vv()))
                                        .unwrap(),
                                )
                                .unwrap();
                        });
                        info_span!("Updates", from = i, to = j).in_scope(|| {
                            b_doc
                                .import(
                                    &a_doc
                                        .export(ExportMode::updates(&b_doc.oplog_vv()))
                                        .unwrap(),
                                )
                                .unwrap();
                        });
                    }
                    1 => {
                        info_span!("Snapshot", from = i, to = j).in_scope(|| {
                            b_doc
                                .import(&a_doc.export(ExportMode::Snapshot).unwrap())
                                .unwrap();
                        });
                        info_span!("Snapshot", from = j, to = i).in_scope(|| {
                            a_doc
                                .import(&b_doc.export(ExportMode::Snapshot).unwrap())
                                .unwrap();
                        });
                    }
                    2 => {
                        info_span!("FastSnapshot", from = i, to = j).in_scope(|| {
                            b_doc
                                .import(&a_doc.export(loro::ExportMode::Snapshot).unwrap())
                                .unwrap();
                        });
                        info_span!("FastSnapshot", from = j, to = i).in_scope(|| {
                            a_doc
                                .import(&b_doc.export(loro::ExportMode::Snapshot).unwrap())
                                .unwrap();
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

                // If one side had pending changes, the first sync may apply
                // them locally without the peer receiving them (pending ops
                // are not included in exports). Retry with Updates until the
                // version vectors converge.
                for round in 0..15 {
                    if a_doc.oplog_vv() == b_doc.oplog_vv() {
                        break;
                    }
                    info_span!("RetryUpdates", round, from = j, to = i).in_scope(|| {
                        let _ = a_doc.import(
                            &b_doc
                                .export(ExportMode::updates(&a_doc.oplog_vv()))
                                .unwrap(),
                        );
                    });
                    info_span!("RetryUpdates", round, from = i, to = j).in_scope(|| {
                        let _ = b_doc.import(
                            &a_doc
                                .export(ExportMode::updates(&b_doc.oplog_vv()))
                                .unwrap(),
                        );
                    });
                }

                if a_doc.oplog_vv() != b_doc.oplog_vv() {
                    panic!(
                        "CRDTFuzzer: sync failed to converge after 16 rounds. \
                         actor {} vv={:?} actor {} vv={:?}",
                        i,
                        a_doc.oplog_vv(),
                        j,
                        b_doc.oplog_vv()
                    );
                }

                a.check_eq(b);
                a.record_history();
                b.record_history();
            }
        }
    }

    fn check_equal_after_updates_sync(&mut self) {
        for i in 0..self.site_num() - 1 {
            for j in i + 1..self.site_num() {
                let _s = info_span!("checking eq after updates sync", ?i, ?j);
                let _g = _s.enter();
                let (a, b) = array_mut_ref!(&mut self.actors, [i, j]);
                let a_shallow = a.loro.is_shallow();
                let b_shallow = b.loro.is_shallow();
                if a_shallow || b_shallow {
                    continue;
                }

                let a_doc = &a.loro;
                let b_doc = &b.loro;
                info_span!("Attach", peer = i).in_scope(|| {
                    a_doc.attach();
                });
                info_span!("Attach", peer = j).in_scope(|| {
                    b_doc.attach();
                });

                for round in 0..16 {
                    if a_doc.oplog_vv() == b_doc.oplog_vv() {
                        break;
                    }
                    info_span!("Updates", round, from = j, to = i).in_scope(|| {
                        a_doc
                            .import(
                                &b_doc
                                    .export(ExportMode::updates(&a_doc.oplog_vv()))
                                    .unwrap(),
                            )
                            .unwrap();
                    });
                    info_span!("Updates", round, from = i, to = j).in_scope(|| {
                        b_doc
                            .import(
                                &a_doc
                                    .export(ExportMode::updates(&b_doc.oplog_vv()))
                                    .unwrap(),
                            )
                            .unwrap();
                    });
                }

                if a_doc.oplog_vv() != b_doc.oplog_vv() {
                    panic!(
                        "CRDTFuzzer: updates sync failed to converge after 16 rounds. \
                         actor {} vv={:?} actor {} vv={:?}",
                        i,
                        a_doc.oplog_vv(),
                        j,
                        b_doc.oplog_vv()
                    );
                }

                a.check_eq(b);
            }
        }
    }

    fn check_tracker(&self) {
        for actor in self.actors.iter() {
            actor.check_tracker();
        }
    }

    fn check_all_actor_state_correctness(&self) {
        for actor in &self.actors {
            actor.loro.attach();
            actor.loro.check_state_correctness_slow();
        }
    }

    fn check_history(&mut self) {
        self.actors[0].check_history();
        // for actor in self.actors.iter_mut() {
        //     actor.check_history();
        // }
    }

    fn prune_history(&mut self, max_entries_per_actor: usize) {
        for actor in self.actors.iter_mut() {
            actor.prune_history(max_entries_per_actor);
        }
    }

    fn site_num(&self) -> usize {
        self.actors.len()
    }

    fn target_index_for(&self, target: ContainerType) -> u8 {
        self.targets
            .iter()
            .position(|ty| *ty == target)
            .unwrap_or(0) as u8
    }
}

fn handle_import_result(e: LoroResult<ImportStatus>) {
    match e {
        Ok(_) => {}
        Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion) => {
            info!("Failed Import Due to ImportUpdatesThatDependsOnOutdatedVersion");
        }
        Err(e) => panic!("{}", e),
    }
}

fn handle_gc_sync_import_result(e: LoroResult<ImportStatus>) -> bool {
    match e {
        Ok(_) => true,
        Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion) => {
            info!("Skipped GC sync due to ImportUpdatesThatDependsOnOutdatedVersion");
            false
        }
        Err(e) => panic!("{}", e),
    }
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
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
                set.insert(ContainerType::Counter);
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
        fuzzer.pre_process(action);
        info_span!("ApplyAction", ?action).in_scope(|| {
            applied.push(action.clone());
            info!("OptionsTable \n{}", (&applied).table());
            // info!("Apply Action {:?}", applied);
            fuzzer.apply_action(action);
        });
    }

    // println!("OpTable \n{}", (&applied).table());
    info_span!("check synced").in_scope(|| {
        fuzzer.check_equal();
    });
    info_span!("check tracker").in_scope(|| {
        fuzzer.check_tracker();
    });
    info_span!("check history").in_scope(|| {
        fuzzer.check_history();
    });
}

pub fn test_multi_sites_with_repro_checks(
    site_num: u8,
    fuzz_targets: Vec<FuzzTarget>,
    actions: &mut [Action],
    check_after_action_indices: &[usize],
) {
    let mut fuzzer = CRDTFuzzer::new(site_num, fuzz_targets);
    let mut checks = check_after_action_indices.iter().copied().peekable();
    for (index, action) in actions.iter_mut().enumerate() {
        fuzzer.pre_process(action);
        info_span!("ApplyAction", ?action).in_scope(|| {
            fuzzer.apply_action(action);
        });

        let applied_len = index + 1;
        while checks
            .peek()
            .is_some_and(|check_after| *check_after <= applied_len)
        {
            checks.next();
            info_span!("repro periodic check", applied_len).in_scope(|| {
                fuzzer.check_all_actor_state_correctness();
            });
        }
    }

    info_span!("check synced").in_scope(|| {
        fuzzer.check_equal();
    });
    info_span!("check all actor state").in_scope(|| {
        fuzzer.check_all_actor_state_correctness();
    });
    info_span!("check history").in_scope(|| {
        fuzzer.check_history();
    });
}

pub fn test_multi_sites_with_repro_check_every(
    site_num: u8,
    fuzz_targets: Vec<FuzzTarget>,
    actions: &mut [Action],
    check_every_actions: usize,
) {
    let check_after_action_indices = if check_every_actions == 0 {
        Vec::new()
    } else {
        (check_every_actions..=actions.len())
            .step_by(check_every_actions)
            .collect()
    };
    test_multi_sites_with_repro_checks(
        site_num,
        fuzz_targets,
        actions,
        &check_after_action_indices,
    );
}

#[derive(Debug, Clone)]
pub struct LongPeerFuzzConfig {
    pub seed: u64,
    pub site_num: u8,
    pub fuzz_targets: Vec<FuzzTarget>,
    pub max_ops: Option<u64>,
    pub duration: Option<Duration>,
    pub sync_barrier_every: u64,
    pub check_every: u64,
    pub history_limit: usize,
    pub full_final_check: bool,
    pub recent_actions: usize,
    pub include_nested_containers: bool,
    pub artifact_dir: PathBuf,
    pub minimize_on_failure: bool,
    pub minimize_time: Duration,
}

impl Default for LongPeerFuzzConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            site_num: 8,
            fuzz_targets: vec![FuzzTarget::All],
            max_ops: Some(10_000),
            duration: None,
            sync_barrier_every: 2_000,
            check_every: 5_000,
            history_limit: 64,
            full_final_check: false,
            recent_actions: 64,
            include_nested_containers: false,
            artifact_dir: PathBuf::from("long_peer_fuzz_artifacts"),
            minimize_on_failure: true,
            minimize_time: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LongPeerFuzzStats {
    pub ops: u64,
    pub elapsed: Duration,
}

struct LongPeerFuzzJournal {
    dir: PathBuf,
    actions: fs::File,
    latest_path: PathBuf,
}

impl LongPeerFuzzJournal {
    fn new(config: &LongPeerFuzzConfig) -> io::Result<Self> {
        fs::create_dir_all(&config.artifact_dir)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let dir = config
            .artifact_dir
            .join(format!("active-seed-{}-{timestamp}", config.seed));
        fs::create_dir_all(&dir)?;

        write_rust_repro_header(
            &dir.join("repro_header.rs"),
            config,
            &format!("repro_long_peer_fuzz_seed_{}_journal", config.seed),
        )?;
        write_rust_repro_footer(&dir.join("repro_footer.rs"), config)?;
        write_journal_readme(&dir, config)?;

        let actions_path = dir.join("actions.rs.inc");
        let actions = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(actions_path)?;
        Ok(Self {
            latest_path: dir.join("latest.txt"),
            dir,
            actions,
        })
    }

    fn record(&mut self, action_index: usize, op: u64, phase: &str, action: &Action) {
        let _ = writeln!(self.actions, "        {},", action_to_rust(action));
        let _ = self.actions.flush();
        let _ = fs::write(
            &self.latest_path,
            format!(
                "action_index={action_index}\nop={op}\nphase={phase}\naction={action:?}\nrepro_dir={}\n",
                self.dir.display()
            ),
        );
    }

    fn finish(&mut self) {
        let _ = self.actions.flush();
        let _ = fs::write(self.dir.join("finished.txt"), "completed without panic\n");
    }
}

pub fn run_long_peer_fuzz(config: LongPeerFuzzConfig) -> LongPeerFuzzStats {
    assert!(
        config.site_num >= 2,
        "long peer fuzz needs at least 2 sites"
    );
    assert!(
        config.max_ops.is_some() || config.duration.is_some(),
        "set max_ops or duration so the runner has a stop condition"
    );

    let start = Instant::now();
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut fuzzer = CRDTFuzzer::new(config.site_num, config.fuzz_targets.clone());
    let mut recent = VecDeque::with_capacity(config.recent_actions);
    let mut actions = Vec::new();
    let mut periodic_check_points = Vec::new();
    let mut journal = match LongPeerFuzzJournal::new(&config) {
        Ok(journal) => Some(journal),
        Err(err) => {
            eprintln!("failed to initialize long_peer_fuzz crash journal: {err}");
            None
        }
    };
    let mut ops = 0;

    while config.max_ops.is_none_or(|max_ops| ops < max_ops)
        && config
            .duration
            .is_none_or(|duration| start.elapsed() < duration)
    {
        ops += 1;
        let action = generate_long_peer_action(&fuzzer, &mut rng, ops, &config);
        actions.push(action.clone());
        if let Some(journal) = journal.as_mut() {
            journal.record(actions.len(), ops, "apply action", &action);
        }
        remember_action(&mut recent, config.recent_actions, ops, &action);
        if let Err(err) = catch_unwind(AssertUnwindSafe(|| {
            apply_raw_long_peer_action(&mut fuzzer, &action);
        })) {
            handle_long_peer_failure(
                &config,
                &actions,
                &periodic_check_points,
                ops,
                "apply action",
                &recent,
                err,
            );
        }
        fuzzer.prune_history(config.history_limit);

        if config.sync_barrier_every != 0 && ops % config.sync_barrier_every == 0 {
            let action = Action::SyncAll;
            actions.push(action.clone());
            if let Some(journal) = journal.as_mut() {
                journal.record(actions.len(), ops, "sync barrier", &action);
            }
            remember_action(&mut recent, config.recent_actions, ops, &action);
            if let Err(err) = catch_unwind(AssertUnwindSafe(|| {
                apply_raw_long_peer_action(&mut fuzzer, &action);
            })) {
                handle_long_peer_failure(
                    &config,
                    &actions,
                    &periodic_check_points,
                    ops,
                    "sync barrier",
                    &recent,
                    err,
                );
            }
            fuzzer.prune_history(config.history_limit);
        }

        if config.check_every != 0 && ops % config.check_every == 0 {
            periodic_check_points.push(actions.len());
            if let Err(err) = catch_unwind(AssertUnwindSafe(|| {
                fuzzer.check_all_actor_state_correctness();
            })) {
                handle_long_peer_failure(
                    &config,
                    &actions,
                    &periodic_check_points,
                    ops,
                    "periodic check",
                    &recent,
                    err,
                );
            }
        }
    }

    let final_phase = if config.full_final_check {
        "full final check"
    } else {
        "quick final check"
    };
    if let Err(err) = catch_unwind(AssertUnwindSafe(|| {
        if config.full_final_check {
            fuzzer.check_equal();
            fuzzer.check_tracker();
            fuzzer.check_history();
        } else {
            fuzzer.check_equal_after_updates_sync();
            fuzzer.check_tracker();
        }
    })) {
        handle_long_peer_failure(
            &config,
            &actions,
            &periodic_check_points,
            ops,
            final_phase,
            &recent,
            err,
        );
    }

    if let Some(journal) = journal.as_mut() {
        journal.finish();
    }

    LongPeerFuzzStats {
        ops,
        elapsed: start.elapsed(),
    }
}

fn remember_action(
    recent: &mut VecDeque<(u64, Action)>,
    recent_actions: usize,
    op: u64,
    action: &Action,
) {
    if recent_actions == 0 {
        return;
    }

    if recent.len() == recent_actions {
        recent.pop_front();
    }
    recent.push_back((op, action.clone()));
}

fn apply_raw_long_peer_action(fuzzer: &mut CRDTFuzzer, raw_action: &Action) {
    let mut action = raw_action.clone();
    fuzzer.pre_process(&mut action);
    fuzzer.apply_action(&action);
}

fn replay_long_peer_actions(
    config: &LongPeerFuzzConfig,
    actions: &[Action],
    check_after_action_indices: &[usize],
) {
    let mut actions = actions.to_vec();
    test_multi_sites_with_repro_checks(
        config.site_num,
        config.fuzz_targets.clone(),
        &mut actions,
        check_after_action_indices,
    );
}

fn handle_long_peer_failure(
    config: &LongPeerFuzzConfig,
    actions: &[Action],
    check_after_action_indices: &[usize],
    op: u64,
    phase: &str,
    recent: &VecDeque<(u64, Action)>,
    err: Box<dyn std::any::Any + Send>,
) -> ! {
    eprintln!(
        "long_peer_fuzz failed: seed={} op={op} phase={phase}",
        config.seed
    );
    eprintln!("recent raw actions:");
    for (recent_op, recent_action) in recent {
        eprintln!("  {recent_op}: {recent_action:?}");
    }

    match write_long_peer_failure_artifacts(config, actions, check_after_action_indices, op, phase)
    {
        Ok(dir) => {
            eprintln!("long_peer_fuzz repro artifacts: {}", dir.display());
        }
        Err(write_err) => {
            eprintln!("failed to write long_peer_fuzz repro artifacts: {write_err}");
        }
    }

    resume_unwind(err);
}

fn write_long_peer_failure_artifacts(
    config: &LongPeerFuzzConfig,
    actions: &[Action],
    check_after_action_indices: &[usize],
    op: u64,
    phase: &str,
) -> io::Result<PathBuf> {
    fs::create_dir_all(&config.artifact_dir)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let case_dir = config.artifact_dir.join(format!(
        "seed-{}-op-{}-actions-{}-{}",
        config.seed,
        op,
        actions.len(),
        timestamp
    ));
    fs::create_dir_all(&case_dir)?;

    let full = actions.to_vec();
    write_action_debug(&case_dir.join("full_actions.txt"), &full)?;
    write_action_debug(&case_dir.join("minimized_actions.txt"), &full)?;
    write_rust_repro(
        &case_dir.join("full_repro.rs"),
        config,
        &full,
        check_after_action_indices,
        &format!("repro_long_peer_fuzz_seed_{}_op_{}_full", config.seed, op),
    )?;
    write_rust_repro(
        &case_dir.join("minimal_repro.rs"),
        config,
        &full,
        check_after_action_indices,
        &format!(
            "repro_long_peer_fuzz_seed_{}_op_{}_minimal",
            config.seed, op
        ),
    )?;
    write_failure_readme(
        &case_dir,
        config,
        actions.len(),
        full.len(),
        check_after_action_indices,
        op,
        phase,
    )?;

    let minimized = if config.minimize_on_failure {
        minimize_long_peer_repro(
            config,
            full.clone(),
            check_after_action_indices,
            config.minimize_time,
        )
    } else {
        full.clone()
    };

    write_action_debug(&case_dir.join("minimized_actions.txt"), &minimized)?;
    write_rust_repro(
        &case_dir.join("minimal_repro.rs"),
        config,
        &minimized,
        &normalize_repro_check_points(check_after_action_indices, minimized.len()),
        &format!(
            "repro_long_peer_fuzz_seed_{}_op_{}_minimal",
            config.seed, op
        ),
    )?;
    write_failure_readme(
        &case_dir,
        config,
        actions.len(),
        minimized.len(),
        check_after_action_indices,
        op,
        phase,
    )?;

    Ok(case_dir)
}

fn write_action_debug(path: &Path, actions: &[Action]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    for (i, action) in actions.iter().enumerate() {
        writeln!(file, "{i}: {action:?}")?;
    }
    Ok(())
}

fn write_failure_readme(
    case_dir: &Path,
    config: &LongPeerFuzzConfig,
    full_len: usize,
    minimized_len: usize,
    check_after_action_indices: &[usize],
    op: u64,
    phase: &str,
) -> io::Result<()> {
    let mut file = fs::File::create(case_dir.join("README.md"))?;
    writeln!(file, "# long_peer_fuzz repro")?;
    writeln!(file)?;
    writeln!(file, "- seed: {}", config.seed)?;
    writeln!(file, "- peers: {}", config.site_num)?;
    writeln!(file, "- failed op: {op}")?;
    writeln!(file, "- failed phase: {phase}")?;
    writeln!(file, "- full actions: {full_len}")?;
    writeln!(file, "- minimized actions: {minimized_len}")?;
    writeln!(
        file,
        "- repro check points: {}",
        check_after_action_indices.len()
    )?;
    writeln!(file, "- targets: {:?}", config.fuzz_targets)?;
    writeln!(
        file,
        "- nested containers: {}",
        config.include_nested_containers
    )?;
    writeln!(file)?;
    writeln!(
        file,
        "`minimal_repro.rs` is an integration-test body. Put it under `crates/fuzz/tests/` and run:"
    )?;
    writeln!(file)?;
    writeln!(file, "```bash")?;
    writeln!(file, "cargo test -p fuzz --test minimal_repro")?;
    writeln!(file, "```")?;
    Ok(())
}

fn write_journal_readme(case_dir: &Path, config: &LongPeerFuzzConfig) -> io::Result<()> {
    let mut file = fs::File::create(case_dir.join("README.md"))?;
    writeln!(file, "# long_peer_fuzz crash journal")?;
    writeln!(file)?;
    writeln!(file, "- seed: {}", config.seed)?;
    writeln!(file, "- peers: {}", config.site_num)?;
    writeln!(file, "- targets: {:?}", config.fuzz_targets)?;
    writeln!(
        file,
        "- nested containers: {}",
        config.include_nested_containers
    )?;
    writeln!(file)?;
    writeln!(
        file,
        "The runner appends one raw action to `actions.rs.inc` before applying it."
    )?;
    writeln!(
        file,
        "If the process aborts before normal repro artifacts are written, rebuild a test with:"
    )?;
    writeln!(file)?;
    writeln!(file, "```bash")?;
    writeln!(
        file,
        "cat repro_header.rs actions.rs.inc repro_footer.rs > journal_repro.rs"
    )?;
    writeln!(
        file,
        "cp journal_repro.rs ../../crates/fuzz/tests/journal_repro.rs"
    )?;
    writeln!(file, "cargo test -p fuzz --test journal_repro")?;
    writeln!(file, "```")?;
    writeln!(file)?;
    writeln!(file, "`latest.txt` records the last action written.")?;
    Ok(())
}

fn minimize_long_peer_repro(
    config: &LongPeerFuzzConfig,
    actions: Vec<Action>,
    check_after_action_indices: &[usize],
    budget: Duration,
) -> Vec<Action> {
    let check_points = normalize_repro_check_points(check_after_action_indices, actions.len());
    if actions.is_empty() || !long_peer_actions_fail(config, &actions, &check_points) {
        return actions;
    }

    let start = Instant::now();
    let mut current = actions;
    let mut chunk = (current.len() / 2).max(1);

    while chunk > 0 && start.elapsed() < budget {
        let mut removed_any = false;
        let mut index = 0;

        while index < current.len() && start.elapsed() < budget {
            let end = (index + chunk).min(current.len());
            let mut candidate = current.clone();
            candidate.drain(index..end);

            let candidate_check_points =
                normalize_repro_check_points(check_after_action_indices, candidate.len());
            if !candidate.is_empty()
                && long_peer_actions_fail(config, &candidate, &candidate_check_points)
            {
                current = candidate;
                removed_any = true;
            } else {
                index += chunk;
            }
        }

        if !removed_any {
            if chunk == 1 {
                break;
            }
            chunk = chunk.div_ceil(2);
        }
    }

    current
}

fn long_peer_actions_fail(
    config: &LongPeerFuzzConfig,
    actions: &[Action],
    check_after_action_indices: &[usize],
) -> bool {
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        replay_long_peer_actions(config, actions, check_after_action_indices);
    }))
    .is_err();
    std::panic::set_hook(old_hook);
    failed
}

fn write_rust_repro(
    path: &Path,
    config: &LongPeerFuzzConfig,
    actions: &[Action],
    check_after_action_indices: &[usize],
    test_name: &str,
) -> io::Result<()> {
    write_rust_repro_header(path, config, test_name)?;
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    for action in actions {
        writeln!(file, "        {},", action_to_rust(action))?;
    }
    write_rust_repro_footer_to(&mut file, config, check_after_action_indices)?;
    Ok(())
}

fn write_rust_repro_header(
    path: &Path,
    _config: &LongPeerFuzzConfig,
    test_name: &str,
) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    writeln!(file, "#![allow(deprecated)]")?;
    writeln!(file)?;
    writeln!(
        file,
        "use fuzz::{{actions::{{ActionWrapper::Generic, GenericAction}}, crdt_fuzzer::{{test_multi_sites_with_repro_checks, Action::*, FuzzTarget, FuzzValue::*}}}};"
    )?;
    writeln!(file, "use loro::ContainerType;")?;
    writeln!(file)?;
    writeln!(file, "#[test]")?;
    writeln!(file, "fn {test_name}() {{")?;
    writeln!(file, "    let mut actions = vec![")?;
    Ok(())
}

fn write_rust_repro_footer(path: &Path, config: &LongPeerFuzzConfig) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    write_rust_repro_footer_to(&mut file, config, &[])
}

fn write_rust_repro_footer_to(
    file: &mut impl Write,
    config: &LongPeerFuzzConfig,
    check_after_action_indices: &[usize],
) -> io::Result<()> {
    writeln!(file, "    ];")?;
    writeln!(
        file,
        "    let check_after_action_indices = {};",
        check_points_to_rust(check_after_action_indices)
    )?;
    writeln!(
        file,
        "    test_multi_sites_with_repro_checks({}, {}, &mut actions, &check_after_action_indices);",
        config.site_num,
        fuzz_targets_to_rust(&config.fuzz_targets)
    )?;
    writeln!(file, "}}")?;
    Ok(())
}

fn normalize_repro_check_points(check_points: &[usize], action_len: usize) -> Vec<usize> {
    let mut normalized: Vec<_> = check_points
        .iter()
        .copied()
        .filter(|point| *point != 0 && *point <= action_len)
        .collect();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}

fn check_points_to_rust(check_points: &[usize]) -> String {
    let points = check_points
        .iter()
        .map(|point| format!("{point}usize"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{points}]")
}

fn action_to_rust(action: &Action) -> String {
    match action {
        Action::Handle {
            site,
            target,
            container,
            action,
        } => format!(
            "Handle {{ site: {site}u8, target: {target}u8, container: {container}u8, action: {} }}",
            action_wrapper_to_rust(action)
        ),
        Action::Checkout { site, to } => {
            format!("Checkout {{ site: {site}u8, to: {to}u32 }}")
        }
        Action::Undo { site, op_len } => {
            format!("Undo {{ site: {site}u8, op_len: {op_len}u32 }}")
        }
        Action::SyncAllUndo { site, op_len } => {
            format!("SyncAllUndo {{ site: {site}u8, op_len: {op_len}u32 }}")
        }
        Action::Sync { from, to } => {
            format!("Sync {{ from: {from}u8, to: {to}u8 }}")
        }
        Action::SyncAll => "SyncAll".to_string(),
        Action::ForkAt { site, to } => {
            format!("ForkAt {{ site: {site}u8, to: {to}u32 }}")
        }
        Action::DiffApply { from, to } => {
            format!("DiffApply {{ from: {from}u8, to: {to}u8 }}")
        }
        Action::Query {
            site,
            target,
            query_type,
        } => {
            format!("Query {{ site: {site}u8, target: {target}u8, query_type: {query_type}u8 }}")
        }
        Action::ExportShallow { site } => {
            format!("ExportShallow {{ site: {site}u8 }}")
        }
        Action::ImportShallow { site, from } => {
            format!("ImportShallow {{ site: {site}u8, from: {from}u8 }}")
        }
        Action::StateOnlyRoundTrip { site } => {
            format!("StateOnlyRoundTrip {{ site: {site}u8 }}")
        }
        Action::Commit { site } => {
            format!("Commit {{ site: {site}u8 }}")
        }
        Action::SetCommitOptions { site, origin, msg } => {
            format!("SetCommitOptions {{ site: {site}u8, origin: {origin}u8, msg: {msg}u8 }}")
        }
    }
}

fn action_wrapper_to_rust(action: &ActionWrapper) -> String {
    match action {
        ActionWrapper::Generic(action) => generic_action_to_rust(action),
        ActionWrapper::Action(_) => {
            panic!("long_peer_fuzz repro writer expects raw Generic actions")
        }
    }
}

fn generic_action_to_rust(action: &GenericAction) -> String {
    format!(
        "Generic(GenericAction {{ value: {}, bool: {}, key: {}u32, pos: {}usize, length: {}usize, prop: {}u64 }})",
        fuzz_value_to_rust(action.value),
        action.bool,
        action.key,
        action.pos,
        action.length,
        action.prop
    )
}

fn fuzz_value_to_rust(value: FuzzValue) -> String {
    match value {
        FuzzValue::I32(value) => format!("I32({value}i32)"),
        FuzzValue::Container(ty) => format!("Container({})", container_type_to_rust(ty)),
    }
}

fn container_type_to_rust(ty: ContainerType) -> String {
    match ty {
        ContainerType::Map => "ContainerType::Map".to_string(),
        ContainerType::List => "ContainerType::List".to_string(),
        ContainerType::Text => "ContainerType::Text".to_string(),
        ContainerType::Tree => "ContainerType::Tree".to_string(),
        ContainerType::MovableList => "ContainerType::MovableList".to_string(),
        ContainerType::Counter => "ContainerType::Counter".to_string(),
        ContainerType::Unknown(value) => format!("ContainerType::Unknown({value})"),
    }
}

fn fuzz_targets_to_rust(targets: &[FuzzTarget]) -> String {
    let targets = targets
        .iter()
        .map(fuzz_target_to_rust)
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{targets}]")
}

fn fuzz_target_to_rust(target: &FuzzTarget) -> &'static str {
    match target {
        FuzzTarget::Map => "FuzzTarget::Map",
        FuzzTarget::List => "FuzzTarget::List",
        FuzzTarget::Text => "FuzzTarget::Text",
        FuzzTarget::Tree => "FuzzTarget::Tree",
        FuzzTarget::MovableList => "FuzzTarget::MovableList",
        FuzzTarget::Counter => "FuzzTarget::Counter",
        FuzzTarget::All => "FuzzTarget::All",
    }
}

fn generate_long_peer_action(
    fuzzer: &CRDTFuzzer,
    rng: &mut StdRng,
    op: u64,
    config: &LongPeerFuzzConfig,
) -> Action {
    let site = rng.gen_range(0..fuzzer.site_num()) as u8;
    match rng.gen_range(0..100) {
        0..=71 => {
            let target_ty = choose_edit_target(rng);
            Action::Handle {
                site,
                target: fuzzer.target_index_for(target_ty),
                container: choose_small_or_boundary_u8(rng),
                action: ActionWrapper::Generic(generate_generic_action(
                    rng,
                    target_ty,
                    op,
                    config.include_nested_containers,
                )),
            }
        }
        72..=82 => {
            let from = rng.gen_range(0..fuzzer.site_num()) as u8;
            let mut to = rng.gen_range(0..fuzzer.site_num()) as u8;
            if from == to {
                to = (to + 1) % fuzzer.site_num() as u8;
            }
            Action::Sync { from, to }
        }
        83..=85 => Action::SyncAll,
        86..=89 => Action::Query {
            site,
            target: fuzzer.target_index_for(choose_query_target(rng)),
            query_type: rng.gen(),
        },
        90..=91 => Action::DiffApply {
            from: site,
            to: ((site as usize + 1 + rng.gen_range(0..fuzzer.site_num() - 1)) % fuzzer.site_num())
                as u8,
        },
        92 => Action::StateOnlyRoundTrip { site },
        93 => Action::ExportShallow { site },
        94 => Action::ForkAt {
            site,
            to: rng.gen(),
        },
        95 => Action::Checkout {
            site,
            to: rng.gen(),
        },
        96 => Action::Undo {
            site,
            op_len: rng.gen_range(1..=8),
        },
        97 => Action::SyncAllUndo {
            site,
            op_len: rng.gen_range(1..=4),
        },
        98 => Action::SetCommitOptions {
            site,
            origin: rng.gen(),
            msg: rng.gen(),
        },
        _ => Action::Commit { site },
    }
}

fn choose_edit_target(rng: &mut StdRng) -> ContainerType {
    match rng.gen_range(0..100) {
        0..=29 => ContainerType::Tree,
        30..=49 => ContainerType::Text,
        50..=64 => ContainerType::MovableList,
        65..=79 => ContainerType::List,
        80..=89 => ContainerType::Map,
        _ => ContainerType::Counter,
    }
}

fn choose_query_target(rng: &mut StdRng) -> ContainerType {
    match rng.gen_range(0..6) {
        0 => ContainerType::Tree,
        1 => ContainerType::Text,
        2 => ContainerType::MovableList,
        3 => ContainerType::List,
        4 => ContainerType::Map,
        _ => ContainerType::Counter,
    }
}

fn generate_generic_action(
    rng: &mut StdRng,
    target: ContainerType,
    op: u64,
    include_nested_containers: bool,
) -> GenericAction {
    GenericAction {
        value: choose_fuzz_value(rng, include_nested_containers),
        bool: rng.gen(),
        key: choose_key(rng, op),
        pos: choose_position(rng, op),
        length: choose_length(rng, op),
        prop: choose_action_prop(rng, target, op),
    }
}

fn choose_fuzz_value(rng: &mut StdRng, include_nested_containers: bool) -> FuzzValue {
    if include_nested_containers && rng.gen_ratio(1, 5) {
        let ty = match rng.gen_range(0..6) {
            0 => ContainerType::Map,
            1 => ContainerType::List,
            2 => ContainerType::Text,
            3 => ContainerType::Tree,
            4 => ContainerType::MovableList,
            _ => ContainerType::Counter,
        };
        FuzzValue::Container(ty)
    } else {
        FuzzValue::I32(rng.gen())
    }
}

fn choose_action_prop(rng: &mut StdRng, target: ContainerType, op: u64) -> u64 {
    let modulo = match target {
        ContainerType::Map => 3,
        ContainerType::List => 5,
        ContainerType::MovableList => 7,
        ContainerType::Text => 9,
        ContainerType::Tree => 10,
        ContainerType::Counter => 2,
        ContainerType::Unknown(_) => 1,
    };
    let residue = if target == ContainerType::Tree || target == ContainerType::Text {
        op % modulo
    } else {
        rng.gen_range(0..modulo)
    };
    rng.gen::<u64>() / modulo * modulo + residue
}

fn choose_key(rng: &mut StdRng, op: u64) -> u32 {
    match rng.gen_range(0..10) {
        0 => 0,
        1 => 1,
        2 => 127,
        3 => 128,
        4 => 255,
        5 => 256,
        6 => op as u32,
        _ => rng.gen(),
    }
}

fn choose_position(rng: &mut StdRng, op: u64) -> usize {
    match rng.gen_range(0..12) {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 63,
        4 => 64,
        5 => 127,
        6 => 128,
        7 => 255,
        8 => 256,
        9 => op as usize,
        _ => rng.gen(),
    }
}

fn choose_length(rng: &mut StdRng, op: u64) -> usize {
    match rng.gen_range(0..12) {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 15,
        5 => 16,
        6 => 31,
        7 => 32,
        8 => 127,
        9 => 128,
        10 => op as usize,
        _ => rng.gen(),
    }
}

fn choose_small_or_boundary_u8(rng: &mut StdRng) -> u8 {
    match rng.gen_range(0..8) {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 127,
        5 => 128,
        6 => 255,
        _ => rng.gen(),
    }
}

pub fn test_multi_sites_with_gc(
    site_num: u8,
    fuzz_targets: Vec<FuzzTarget>,
    actions: &mut [Action],
) {
    if actions.is_empty() {
        return;
    }

    ensure_cov::notify_cov("fuzz_gc");
    let mut fuzzer = CRDTFuzzer::new(site_num, fuzz_targets);
    let mut applied = Vec::new();
    let target_trimmed_index = actions.len() / 2;
    for (i, action) in actions.iter_mut().enumerate() {
        fuzzer.pre_process(action);
        info_span!("ApplyAction", ?action).in_scope(|| {
            applied.push(action.clone());
            info!("OptionsTable \n{}", (&applied).table());
            // info!("Apply Action {:?}", applied);
            fuzzer.apply_action(action);
        });

        if i == target_trimmed_index {
            info_span!("GC 1 => 0").in_scope(|| {
                fuzzer.actors[1].loro.attach();
                let f = fuzzer.actors[1].loro.oplog_frontiers();
                if !f.is_empty() {
                    ensure_cov::notify_cov("export_shallow_snapshot");
                    let bytes = fuzzer.actors[1]
                        .loro
                        .export(loro::ExportMode::shallow_snapshot(&f));
                    handle_gc_sync_import_result(fuzzer.actors[0].loro.import(&bytes.unwrap()));
                }
            })
        }
    }

    // println!("OpTable \n{}", (&applied).table());
    info_span!("check synced").in_scope(|| {
        // peer 0 is a special case here
        let this = &mut fuzzer;
        for i in 1..this.site_num() - 1 {
            for j in i + 1..this.site_num() {
                let _s = info_span!("checking eq", ?i, ?j);
                let _g = _s.enter();
                let (a, b) = array_mut_ref!(&mut this.actors, [i, j]);
                let a_doc = &mut a.loro;
                let b_doc = &mut b.loro;
                info_span!("Attach", peer = i).in_scope(|| {
                    a_doc.attach();
                });
                info_span!("Attach", peer = j).in_scope(|| {
                    b_doc.attach();
                });
                let mut can_check_eq = true;
                match (i + j) % 4 {
                    0 => {
                        let synced = info_span!("Updates", from = j, to = i).in_scope(|| {
                            handle_gc_sync_import_result(
                                a_doc.import(
                                    &b_doc
                                        .export(ExportMode::updates(&a_doc.oplog_vv()))
                                        .unwrap(),
                                ),
                            )
                        });
                        can_check_eq &= synced;
                        if can_check_eq {
                            let synced = info_span!("Updates", from = i, to = j).in_scope(|| {
                                handle_gc_sync_import_result(
                                    b_doc.import(
                                        &a_doc
                                            .export(ExportMode::updates(&b_doc.oplog_vv()))
                                            .unwrap(),
                                    ),
                                )
                            });
                            can_check_eq &= synced;
                        }
                    }
                    1 => {
                        let synced = info_span!("Snapshot", from = i, to = j).in_scope(|| {
                            handle_gc_sync_import_result(
                                b_doc.import(&a_doc.export(ExportMode::Snapshot).unwrap()),
                            )
                        });
                        can_check_eq &= synced;
                        if can_check_eq {
                            let synced = info_span!("Snapshot", from = j, to = i).in_scope(|| {
                                handle_gc_sync_import_result(
                                    a_doc.import(&b_doc.export(ExportMode::Snapshot).unwrap()),
                                )
                            });
                            can_check_eq &= synced;
                        }
                    }
                    2 => {
                        let synced = info_span!("FastSnapshot", from = i, to = j).in_scope(|| {
                            handle_gc_sync_import_result(
                                b_doc.import(&a_doc.export(loro::ExportMode::Snapshot).unwrap()),
                            )
                        });
                        can_check_eq &= synced;
                        if can_check_eq {
                            let synced =
                                info_span!("FastSnapshot", from = j, to = i).in_scope(|| {
                                    handle_gc_sync_import_result(
                                        a_doc.import(
                                            &b_doc.export(loro::ExportMode::Snapshot).unwrap(),
                                        ),
                                    )
                                });
                            can_check_eq &= synced;
                        }
                    }
                    _ => {
                        let synced = info_span!("JsonFormat", from = i, to = j).in_scope(|| {
                            let a_json =
                                a_doc.export_json_updates(&b_doc.oplog_vv(), &a_doc.oplog_vv());
                            handle_gc_sync_import_result(b_doc.import_json_updates(a_json))
                        });
                        can_check_eq &= synced;
                        if can_check_eq {
                            let synced =
                                info_span!("JsonFormat", from = j, to = i).in_scope(|| {
                                    let b_json = b_doc
                                        .export_json_updates(&a_doc.oplog_vv(), &b_doc.oplog_vv());
                                    handle_gc_sync_import_result(a_doc.import_json_updates(b_json))
                                });
                            can_check_eq &= synced;
                        }
                    }
                }

                if can_check_eq && a.loro.oplog_vv() != b.loro.oplog_vv() {
                    // There is chance this happens when a pending update is applied because of the previous import
                    let a_doc = &mut a.loro;
                    let b_doc = &mut b.loro;
                    let synced = info_span!("Updates", from = j, to = i).in_scope(|| {
                        handle_gc_sync_import_result(
                            a_doc.import(
                                &b_doc
                                    .export(ExportMode::updates(&a_doc.oplog_vv()))
                                    .unwrap(),
                            ),
                        )
                    });
                    can_check_eq &= synced;
                    if can_check_eq {
                        let synced = info_span!("Updates", from = i, to = j).in_scope(|| {
                            handle_gc_sync_import_result(
                                b_doc.import(
                                    &a_doc
                                        .export(ExportMode::updates(&b_doc.oplog_vv()))
                                        .unwrap(),
                                ),
                            )
                        });
                        can_check_eq &= synced;
                    }
                }

                if can_check_eq {
                    a.check_eq(b);
                    a.record_history();
                    b.record_history();
                }
            }
        }

        info_span!("SyncWithGC").in_scope(|| {
            let (a, b) = array_mut_ref!(&mut this.actors, [0, 1]);
            a.loro.attach();
            b.loro.attach();
            let synced = info_span!("0 => 1").in_scope(|| {
                handle_gc_sync_import_result(
                    b.loro.import(
                        &a.loro
                            .export(ExportMode::updates(&b.loro.oplog_vv()))
                            .unwrap(),
                    ),
                )
            });
            if !synced {
                return;
            }
            let result = info_span!("1 => 0").in_scope(|| {
                a.loro.import(
                    &b.loro
                        .export(ExportMode::updates(&a.loro.oplog_vv()))
                        .unwrap(),
                )
            });
            match result {
                Ok(_) => {
                    a.check_eq(b);
                    a.record_history();
                    b.record_history();
                }
                Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion) => {}
                Err(e) => {
                    panic!("{}", e)
                }
            }
        });
    });
    info_span!("check tracker").in_scope(|| {
        fuzzer.check_tracker();
    });
    info_span!("check history").in_scope(|| {
        fuzzer.check_history();
    });

    static COUNT: AtomicUsize = AtomicUsize::new(0);
    if COUNT
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .is_multiple_of(1_000)
    {
        let must_meet = [
            "fuzz_gc",
            "export_shallow_snapshot",
            "shallow_snapshot::need_calc",
            "shallow_snapshot::dont_need_calc",
            "loro_internal::history_cache::find_text_chunks_in",
            "loro_internal::history_cache::find_list_chunks_in",
            "loro_internal::import",
            "loro_internal::import::fast_snapshot::decode_snapshot",
            "loro_internal::import::snapshot",
            "loro_internal::import::snapshot::gc",
            "loro_internal::import::snapshot::normal",
            "loro_internal::history_cache::init_cache_by_visit_all_change_slow::visit_gc",
            "kv-store::SstableIter::new_scan::start included",
            "kv-store::SstableIter::new_scan::start excluded",
            "kv-store::SstableIter::new_scan::start unbounded",
            "kv-store::SstableIter::new_scan::end included",
            "kv-store::SstableIter::new_scan::end excluded",
            "kv-store::SstableIter::new_scan::end unbounded",
            "kv-store::SstableIter::new_scan::end unbounded equal",
            "loro_internal::handler::movable_list_apply_delta::process_replacements::mov_0",
            "loro_internal::handler::movable_list_apply_delta::process_replacements::mov_1",
        ];
        for v in must_meet {
            let count = ensure_cov::get_cov_for(v);
            if count == 0 {
                println!("[COV] FAILED {}", v)
            } else {
                println!("[COV] HIT    {} - {}", v, count)
            }
        }
    }
}

pub fn minify_error<T, F, N>(site_num: u8, f: F, normalize: N, actions: Vec<T>)
where
    F: Fn(u8, &mut [T]) + Send + Sync + 'static,
    N: Fn(u8, &mut [T]) -> Vec<T>,
    T: Clone + Debug + Send + 'static,
{
    println!("Minifying...");
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
        // println!("{:?}", _info);
    }));

    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
    #[allow(clippy::redundant_clone)]
    let mut actions_clone = actions.clone();
    let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
    eprintln!("Initializing");
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

    eprintln!("Started!");
    let minified = Arc::new(Mutex::new(actions.clone()));
    let candidates = Arc::new(Mutex::new(VecDeque::new()));
    println!("Setup candidates...");
    for i in 0..actions.len() {
        let mut new = actions.clone();
        new.remove(i);
        candidates.lock().unwrap().push_back(new);
    }

    println!("Minifying...");
    let start = Instant::now();
    // Get the number of logical cores available on the system
    let num_cores = num_cpus::get() / 2;
    let f = Arc::new(f);
    println!("start with {} threads", num_cores);
    let mut threads = Vec::new();
    for _i in 0..num_cores {
        let candidates = candidates.clone();
        let minified = minified.clone();
        let f = f.clone();
        threads.push(thread::spawn(move || {
            loop {
                let candidate = {
                    let Some(candidate) = candidates.lock().unwrap().pop_back() else {
                        return;
                    };
                    candidate
                };

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
                    let mut candidates = candidates.lock().unwrap();
                    let mut minified = minified.lock().unwrap();
                    for i in 0..candidate.len() {
                        let mut new = candidate.clone();
                        new.remove(i);
                        candidates.push_back(new);
                    }
                    if candidate.len() < minified.len() {
                        *minified = candidate;
                        println!("New min len={}", minified.len());
                    }

                    if candidates.len() > 60 {
                        candidates.drain(0..30);
                    }
                }

                if start.elapsed().as_secs() > 10 && minified.lock().unwrap().len() <= 4 {
                    break;
                }
                if start.elapsed().as_secs() > 60 {
                    break;
                }
            }
        }));
    }

    for thread in threads.into_iter() {
        thread.join().unwrap_or_default();
    }

    let minified = normalize(site_num, &mut minified.lock().unwrap());
    println!(
        "Old Length {}, New Length {}",
        actions.len(),
        minified.len()
    );
    dbg!(&minified);
    if actions.len() > minified.len() {
        minify_error(
            site_num,
            match Arc::try_unwrap(f) {
                Ok(f) => f,
                Err(_) => panic!(),
            },
            normalize,
            minified,
        );
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
    while current_index > 0 {
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
