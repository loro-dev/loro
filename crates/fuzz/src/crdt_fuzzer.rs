use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
    sync::{atomic::AtomicUsize, Arc, Mutex},
    thread,
    time::Instant,
};

use arbitrary::Arbitrary;
use loro::{
    ContainerType, ExportMode, Frontiers, ImportStatus, LoroDoc, LoroError, LoroResult, TreeID,
};
use rustc_hash::FxHashSet;
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
                    let _ = b.loro.apply_diff(diff);
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
                            assert_eq!(new_doc.get_deep_value(), actor.loro.get_deep_value());
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
                let a_doc = &mut a.loro;
                let b_doc = &mut b.loro;
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

#[derive(Eq, Hash, PartialEq, Clone)]
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
