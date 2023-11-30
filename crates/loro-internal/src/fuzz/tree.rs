use std::{
    collections::HashSet,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use arbitrary::Arbitrary;
use debug_log::debug_dbg;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro_common::{LoroError, LoroTreeError, TreeID, ID};
use tabled::{TableIteratorExt, Tabled};

#[allow(unused_imports)]
use crate::{
    array_mut_ref, container::ContainerID, delta::DeltaItem, event::UnresolvedDiff, id::PeerID,
    ContainerType, LoroValue,
};
use crate::{
    delta::TreeValue,
    event::{Diff, Index},
    handler::TreeHandler,
    loro::LoroDoc,
    value::{unresolved_to_collection, ToJson},
    version::Frontiers,
    ApplyDiff, ListHandler, MapHandler, TextHandler,
};

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Tree {
        site: u8,
        container_idx: u8,
        action: TreeAction,
        target: (u64, i32),
        parent: (u64, i32),
    },
    Sync {
        from: u8,
        to: u8,
    },
    SyncAll,
}

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq)]
pub enum TreeAction {
    Create,
    Move,
    Delete,
    Meta,
}

impl Debug for TreeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeAction::Create => f.write_str("TreeAction::Create"),
            TreeAction::Move => f.write_str("TreeAction::Move"),
            TreeAction::Delete => f.write_str("TreeAction::Delete"),
            TreeAction::Meta => f.write_str("TreeAction::Meta"),
        }
    }
}

struct Actor {
    peer: PeerID,
    loro: LoroDoc,
    value_tracker: Arc<Mutex<LoroValue>>,
    tree_tracker: Arc<Mutex<Vec<LoroValue>>>,
    map_containers: Vec<MapHandler>,
    list_containers: Vec<ListHandler>,
    text_containers: Vec<TextHandler>,
    tree_containers: Vec<TreeHandler>,
    history: FxHashMap<Vec<ID>, LoroValue>,
}

impl Actor {
    fn new(id: PeerID) -> Self {
        let app = LoroDoc::new();
        app.set_peer_id(id).unwrap();
        let mut actor = Actor {
            peer: id,
            loro: app,
            value_tracker: Arc::new(Mutex::new(LoroValue::Map(Default::default()))),
            tree_tracker: Default::default(),
            map_containers: Default::default(),
            list_containers: Default::default(),
            text_containers: Default::default(),
            tree_containers: Default::default(),
            history: Default::default(),
        };

        let root_value = Arc::clone(&actor.value_tracker);
        actor.loro.subscribe_root(Arc::new(move |event| {
            let mut root_value = root_value.lock().unwrap();
            debug_dbg!(&event);
            // if id == 0 {
            //     println!("\nbefore {:?}", root_value);
            //     println!("\ndiff {:?}", event);
            // }
            root_value.apply(
                &event.container.path.iter().map(|x| x.1.clone()).collect(),
                &[event.container.diff.clone()],
            );
            // if id == 0 {
            //     println!("\nafter {:?}", root_value);
            // }
        }));

        let tree = Arc::clone(&actor.tree_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("tree", ContainerType::Tree),
            Arc::new(move |event| {
                if event.from_children {
                    // meta
                    let Index::Node(target) = event.container.path.last().unwrap().1 else {
                        unreachable!()
                    };
                    let mut tree = tree.lock().unwrap();
                    let Some(map) = tree.iter_mut().find(|x| {
                        let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                        id.as_ref() == &target.to_string()
                    }) else {
                        //  maybe delete tree node first
                        return;
                    };
                    let map = Arc::make_mut(map.as_map_mut().unwrap());
                    let meta = map.get_mut("meta").unwrap();
                    let meta = Arc::make_mut(meta.as_map_mut().unwrap());
                    if let Diff::NewMap(update) = &event.container.diff {
                        for (key, value) in update.updated.iter() {
                            match &value.value {
                                Some(value) => {
                                    meta.insert(key.to_string(), unresolved_to_collection(value));
                                }
                                None => {
                                    meta.remove(&key.to_string());
                                }
                            }
                        }
                    }

                    return;
                }
                let mut tree = tree.lock().unwrap();
                if let Diff::Tree(tree_diff) = &event.container.diff {
                    let mut v = TreeValue(&mut tree);
                    v.apply_diff(tree_diff);
                } else {
                    debug_dbg!(&event.container);
                    unreachable!()
                }
            }),
        );

        actor
            .text_containers
            .push(actor.loro.txn().unwrap().get_text("text"));
        actor
            .map_containers
            .push(actor.loro.txn().unwrap().get_map("map"));
        actor
            .list_containers
            .push(actor.loro.txn().unwrap().get_list("list"));
        actor
            .tree_containers
            .push(actor.loro.txn().unwrap().get_tree("tree"));
        actor
    }

    fn record_history(&mut self) {
        let f = self.loro.oplog_frontiers();
        let mut value = self.loro.get_deep_value();
        Arc::make_mut(
            Arc::make_mut(value.as_map_mut().unwrap())
                .get_mut("tree")
                .unwrap()
                .as_list_mut()
                .unwrap(),
        )
        .sort_by_key(|x| {
            let id = x.as_map().unwrap().get("id").unwrap();
            id.clone().into_string().unwrap()
        });
        let mut ids: Vec<ID> = f.iter().cloned().collect();
        ids.sort_by_key(|x| x.peer);
        self.history.insert(ids, value);
    }
}

#[derive(Arbitrary, Clone, Debug, PartialEq, Eq)]
pub enum FuzzValue {
    Null,
    I32(i32),
    Container(ContainerType),
}

impl From<FuzzValue> for LoroValue {
    fn from(v: FuzzValue) -> Self {
        match v {
            FuzzValue::Null => LoroValue::Null,
            FuzzValue::I32(i) => LoroValue::I32(i),
            FuzzValue::Container(_) => unreachable!(),
        }
    }
}

impl Tabled for Action {
    const LENGTH: usize = 5;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Sync { from, to } => vec![
                "sync".into(),
                format!("{} with {}", from, to).into(),
                "".into(),
                "".into(),
                "".into(),
            ],
            Action::SyncAll => vec!["sync all".into(), "".into(), "".into(), "".into()],
            Action::Tree {
                site,
                container_idx,
                action,
                target,
                parent,
            } => {
                let action_str = match action {
                    TreeAction::Create => "Create".to_string(),
                    TreeAction::Move => format!("Move to {parent:?}"),
                    TreeAction::Delete => "Delete".to_string(),
                    TreeAction::Meta => "Meta".to_string(),
                };
                vec![
                    "tree".into(),
                    format!("{}", site).into(),
                    format!("{}", container_idx).into(),
                    format!("{:?}", target).into(),
                    action_str.into(),
                ]
            }
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "type".into(),
            "site".into(),
            "container".into(),
            "prop".into(),
            "value".into(),
        ]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&mut self, action: &mut Action);
}

impl Actionable for Vec<Actor> {
    fn preprocess(&mut self, action: &mut Action) {
        let max_users = self.len() as u8;
        match action {
            Action::Sync { from, to } => {
                *from %= max_users;
                *to %= max_users;
                if to == from {
                    *to = (*to + 1) % max_users;
                }
            }
            Action::SyncAll => {}
            Action::Tree {
                site,
                container_idx,
                target: (target_peer, target_counter),
                parent: (parent_peer, parent_counter),
                action,
            } => {
                *site %= max_users;
                *container_idx %= self[*site as usize].tree_containers.len().max(1) as u8;
                if let Some(tree) = self[*site as usize]
                    .tree_containers
                    .get(*container_idx as usize)
                {
                    let nodes = tree.nodes();
                    let tree_num = nodes.len();
                    let mut max_counter_mapping = FxHashMap::default();
                    for TreeID { peer, counter } in nodes.clone() {
                        if let Some(c) = max_counter_mapping.get_mut(&peer) {
                            *c = counter.max(*c);
                        } else {
                            max_counter_mapping.insert(peer, counter);
                        }
                    }
                    if tree_num == 0
                        || tree_num < 2
                            && (matches!(action, TreeAction::Move)
                                || matches!(action, TreeAction::Meta))
                    {
                        *action = TreeAction::Create;
                    } else if tree_num >= 255 && matches!(action, TreeAction::Create) {
                        *action = TreeAction::Move;
                    }

                    match action {
                        TreeAction::Create => {
                            let actor = &mut self[*site as usize];
                            let txn = actor.loro.txn().unwrap();
                            let id = txn.next_id();
                            *target_peer = id.peer;
                            *target_counter = id.counter;
                        }
                        TreeAction::Move => {
                            let target_idx = *target_peer as usize % tree_num;
                            let mut parent_idx = *parent_peer as usize % tree_num;
                            while target_idx == parent_idx {
                                parent_idx = (parent_idx + 1) % tree_num;
                            }
                            *target_peer = nodes[target_idx].peer;
                            *target_counter = nodes[target_idx].counter;
                            *parent_peer = nodes[parent_idx].peer;
                            *parent_counter = nodes[parent_idx].counter;
                        }
                        TreeAction::Delete => {
                            let target_idx = *target_peer as usize % tree_num;
                            *target_peer = nodes[target_idx].peer;
                            *target_counter = nodes[target_idx].counter;
                        }
                        TreeAction::Meta => {
                            let target_idx = *target_peer as usize % tree_num;
                            *target_peer = nodes[target_idx].peer;
                            *target_counter = nodes[target_idx].counter;
                        }
                    }
                } else {
                    *target_peer = *site as u64;
                    *target_counter = 0;
                    *parent_peer = 0;
                    *parent_counter = 0;
                    *action = TreeAction::Create;
                }
            }
        }
    }

    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Sync { from, to } => {
                let (a, b) = array_mut_ref!(self, [*from as usize, *to as usize]);
                let mut visited = HashSet::new();
                a.map_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.list_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.text_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.tree_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });

                a.loro
                    .import(&b.loro.export_from(&a.loro.oplog_vv()))
                    .unwrap();
                b.loro
                    .import(&a.loro.export_from(&b.loro.oplog_vv()))
                    .unwrap();

                if a.peer == 1 {
                    a.record_history();
                }

                b.map_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.map_containers.push(a.loro.txn().unwrap().get_map(id))
                    }
                });
                b.list_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.list_containers.push(a.loro.txn().unwrap().get_list(id))
                    }
                });
                b.text_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.text_containers.push(a.loro.txn().unwrap().get_text(id))
                    }
                });
                b.tree_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.tree_containers.push(a.loro.txn().unwrap().get_tree(id))
                    }
                });

                b.map_containers = a
                    .map_containers
                    .iter()
                    .map(|x| b.loro.get_map(x.id()))
                    .collect();
                b.list_containers = a
                    .list_containers
                    .iter()
                    .map(|x| b.loro.get_list(x.id()))
                    .collect();
                b.text_containers = a
                    .text_containers
                    .iter()
                    .map(|x| b.loro.get_text(x.id()))
                    .collect();
                b.tree_containers = a
                    .tree_containers
                    .iter()
                    .map(|x| b.loro.get_tree(x.id()))
                    .collect();
            }
            Action::SyncAll => {
                let mut visited = HashSet::new();
                let a = &mut self[0];
                a.map_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.list_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.text_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.tree_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });

                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    a.loro
                        .import(&b.loro.export_from(&a.loro.oplog_vv()))
                        .unwrap();
                    b.map_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.map_containers.push(a.loro.get_map(id))
                        }
                    });
                    b.list_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.list_containers.push(a.loro.get_list(id))
                        }
                    });
                    b.text_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.text_containers.push(a.loro.get_text(id))
                        }
                    });
                    b.tree_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.tree_containers.push(a.loro.get_tree(id))
                        }
                    });
                }

                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    b.loro
                        .import(&a.loro.export_from(&b.loro.oplog_vv()))
                        .unwrap();
                    b.map_containers = a
                        .map_containers
                        .iter()
                        .map(|x| b.loro.get_map(x.id()))
                        .collect();
                    b.list_containers = a
                        .list_containers
                        .iter()
                        .map(|x| b.loro.get_list(x.id()))
                        .collect();
                    b.text_containers = a
                        .text_containers
                        .iter()
                        .map(|x| b.loro.get_text(x.id()))
                        .collect();
                    b.tree_containers = a
                        .tree_containers
                        .iter()
                        .map(|x| b.loro.get_tree(x.id()))
                        .collect();
                }

                self[1].record_history();
            }
            Action::Tree {
                site,
                container_idx,
                action,
                target: (target_peer, target_counter),
                parent: (parent_peer, parent_counter),
            } => {
                let actor = &mut self[*site as usize];
                let container = actor.tree_containers.get_mut(*container_idx as usize);
                let container = if let Some(container) = container {
                    container
                } else {
                    let tree = actor.loro.get_tree("tree");
                    actor.tree_containers.push(tree);
                    &mut actor.tree_containers[0]
                };
                let mut txn = actor.loro.txn().unwrap();

                match action {
                    TreeAction::Create => {
                        container.create_with_txn(&mut txn, None).unwrap();
                    }
                    TreeAction::Move => {
                        match container.mov_with_txn(
                            &mut txn,
                            TreeID {
                                peer: *target_peer,
                                counter: *target_counter,
                            },
                            TreeID {
                                peer: *parent_peer,
                                counter: *parent_counter,
                            },
                        ) {
                            Ok(_) => {}
                            Err(err) => {
                                if !matches!(
                                    err,
                                    LoroError::TreeError(LoroTreeError::CyclicMoveError)
                                ) {
                                    panic!("{}", err)
                                }
                            }
                        }
                    }
                    TreeAction::Delete => {
                        container
                            .delete_with_txn(
                                &mut txn,
                                TreeID {
                                    peer: *target_peer,
                                    counter: *target_counter,
                                },
                            )
                            .unwrap();
                    }
                    TreeAction::Meta => {
                        let key = parent_peer.to_string();
                        let value = *parent_counter;
                        let meta = container
                            .get_meta(TreeID {
                                peer: *target_peer,
                                counter: *target_counter,
                            })
                            .unwrap();
                        meta.insert_with_txn(&mut txn, &key, value.into()).unwrap();
                    }
                }

                drop(txn);
                if actor.peer == 1 {
                    actor.record_history();
                }
            }
        }
    }
}

#[allow(unused)]
fn assert_value_eq(a: &LoroValue, b: &LoroValue) {
    match (a, b) {
        (LoroValue::Map(a), LoroValue::Map(b)) => {
            for (k, v) in a.iter() {
                let is_empty = match v {
                    LoroValue::String(s) => s.is_empty(),
                    LoroValue::List(l) => l.is_empty(),
                    LoroValue::Map(m) => m.is_empty(),
                    _ => false,
                };
                if is_empty {
                    continue;
                }
                assert_value_eq(v, b.get(k).unwrap());
            }

            for (k, v) in b.iter() {
                let is_empty = match v {
                    LoroValue::String(s) => s.is_empty(),
                    LoroValue::List(l) => l.is_empty(),
                    LoroValue::Map(m) => m.is_empty(),
                    _ => false,
                };
                if is_empty {
                    continue;
                }
                assert_value_eq(v, a.get(k).unwrap());
            }
        }
        (a, b) => assert_eq!(a, b),
    }
}

fn check_eq(a_actor: &mut Actor, b_actor: &mut Actor) {
    let a_doc = &mut a_actor.loro;
    let b_doc = &mut b_actor.loro;
    let mut a_result = a_doc.get_state_deep_value();
    let mut b_result = b_doc.get_state_deep_value();
    let mut a_value = a_actor.value_tracker.lock().unwrap();

    if let Some(tree) = Arc::make_mut(a_result.as_map_mut().unwrap()).get_mut("tree") {
        Arc::make_mut(tree.as_list_mut().unwrap()).sort_by_key(|x| {
            let id = x.as_map().unwrap().get("id").unwrap();
            id.clone().into_string().unwrap()
        });
    }
    if let Some(tree) = Arc::make_mut(b_result.as_map_mut().unwrap()).get_mut("tree") {
        Arc::make_mut(tree.as_list_mut().unwrap()).sort_by_key(|x| {
            let id = x.as_map().unwrap().get("id").unwrap();
            id.clone().into_string().unwrap()
        });
    }
    if let Some(tree) = Arc::make_mut(a_value.as_map_mut().unwrap()).get_mut("tree") {
        Arc::make_mut(tree.as_list_mut().unwrap()).sort_by_key(|x| {
            let id = x.as_map().unwrap().get("id").unwrap();
            id.clone().into_string().unwrap()
        });
    }
    debug_log::debug_log!("{}", a_result.to_json_pretty());
    assert_eq!(&a_result, &b_result);
    assert_value_eq(&a_result, &a_value);

    let a = a_doc.get_tree("tree");
    let mut value_a = a.get_deep_value().into_list().unwrap();
    let mut tracker_a = a_actor.tree_tracker.lock().unwrap();

    Arc::make_mut(&mut value_a).sort_by_key(|x| {
        let id = x.as_map().unwrap().get("id").unwrap();
        id.clone().into_string().unwrap()
    });
    tracker_a.sort_by_key(|x| {
        let id = x.as_map().unwrap().get("id").unwrap();
        id.clone().into_string().unwrap()
    });

    assert_eq!(&*value_a, &*tracker_a);
}

fn check_synced(sites: &mut [Actor]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log::group!("checking {} with {}", i, j);
            let (a, b) = array_mut_ref!(sites, [i, j]);
            let a_doc = &mut a.loro;
            let b_doc = &mut b.loro;
            if (i + j) % 2 == 0 {
                debug_log::group!("Updates {} to {}", j, i);
                a_doc.import(&b_doc.export_from(&a_doc.oplog_vv())).unwrap();
                debug_log::group_end!();
                debug_log::group!("Updates {} to {}", i, j);
                b_doc.import(&a_doc.export_from(&b_doc.oplog_vv())).unwrap();
                debug_log::group_end!();
            } else {
                debug_log::group!("Snapshot {} to {}", j, i);
                a_doc.import(&b_doc.export_snapshot()).unwrap();
                debug_log::group_end!();
                debug_log::group!("Snapshot {} to {}", i, j);
                b_doc.import(&a_doc.export_snapshot()).unwrap();
                debug_log::group_end!();
            }
            check_eq(a, b);
            debug_log::group_end!();
            if i == 1 {
                a.record_history();
            }
            if j == 1 {
                b.record_history();
            }
        }
    }
}

fn check_history(actor: &mut Actor) {
    assert!(!actor.history.is_empty());
    for (c, (f, v)) in actor.history.iter().enumerate() {
        let f = Frontiers::from(f);
        // println!("\nfrom {:?} checkout {:?}", actor.loro.oplog_vv(), f);
        // println!("before state {:?}", actor.loro.get_deep_value());
        actor.loro.checkout(&f).unwrap();
        let mut actual = actor.loro.get_deep_value();
        Arc::make_mut(
            Arc::make_mut(actual.as_map_mut().unwrap())
                .get_mut("tree")
                .unwrap()
                .as_list_mut()
                .unwrap(),
        )
        .sort_by_key(|x| {
            let id = x.as_map().unwrap().get("id").unwrap();
            id.clone().into_string().unwrap()
        });
        assert_eq!(v, &actual, "Version mismatched at {:?}, cnt={}", f, c);
    }
}

pub fn normalize(site_num: u8, actions: &mut [Action]) -> Vec<Action> {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(Actor::new(i as u64));
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        let sites_ptr: usize = &mut sites as *mut _ as usize;
        #[allow(clippy::blocks_in_if_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: Test
            let sites = unsafe { &mut *(sites_ptr as *mut Vec<_>) };
            sites.apply_action(&action.clone());
        })
        .is_err()
        {
            break;
        }
    }

    println!("{}", applied.clone().table());
    applied
}

pub fn test_multi_sites(site_num: u8, actions: &mut [Action]) {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(Actor::new(i as u64));
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        debug_log::debug_log!("\n{}", (&applied).table());
        sites.apply_action(action);
    }

    debug_log::group!("check synced");
    check_synced(&mut sites);
    debug_log::group_end!();
    check_history(&mut sites[1]);
}

#[cfg(test)]
mod failed_tests {
    use crate::fuzz::minify_error;

    use super::normalize;
    use super::test_multi_sites;
    use super::Action::*;

    #[test]
    fn empty() {
        test_multi_sites(2, &mut [])
    }

    use super::TreeAction;
    #[test]
    fn tree() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 31,
                    container_idx: 31,
                    action: TreeAction::Create,
                    target: (7225181607844773663, 1684298240),
                    parent: (7015277009875896676, -1726003871),
                },
                Tree {
                    site: 31,
                    container_idx: 37,
                    action: TreeAction::Delete,
                    target: (729335883198625631, 655616),
                    parent: (3761688988315248897, 875836468),
                },
            ],
        )
    }

    #[test]
    fn tree_meta() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Meta,
                    target: (1297037105016282879, -65536),
                    parent: (71810203921678335, 16842592),
                },
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (7523378309278230528, -1),
                    parent: (7040879940371245160, -1634364121),
                },
                Tree {
                    site: 97,
                    container_idx: 68,
                    action: TreeAction::Move,
                    target: (2369443269732456, 2130710784),
                    parent: (10995706711611801855, -32104),
                },
                Tree {
                    site: 30,
                    container_idx: 255,
                    action: TreeAction::Meta,
                    target: (280710472564735, -256),
                    parent: (8584986789609525, -16777216),
                },
            ],
        )
    }

    #[test]
    fn tree4() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 154,
                    container_idx: 68,
                    action: TreeAction::Delete,
                    target: (10539624087947575836, -48060),
                    parent: (4919338167840669695, 1153154047),
                },
                Tree {
                    site: 68,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (75541725773824, 0),
                    parent: (17870283321406128128, 35454975),
                },
                Tree {
                    site: 68,
                    container_idx: 146,
                    action: TreeAction::Delete,
                    target: (4919338167840669695, 1147553023),
                    parent: (574490427466448708, 1153153792),
                },
                Tree {
                    site: 68,
                    container_idx: 255,
                    action: TreeAction::Delete,
                    target: (4971768375647666275, 48530),
                    parent: (6, 541327360),
                },
                SyncAll,
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (18446735572701395712, 1145307676),
                    parent: (18446537660035253316, -131465217),
                },
                Tree {
                    site: 68,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (2244103232290815, -12272709),
                    parent: (18444773745722954464, 1145324799),
                },
                SyncAll,
                Tree {
                    site: 255,
                    container_idx: 224,
                    action: TreeAction::Delete,
                    target: (10539830502813073407, -506812),
                    parent: (4919131752984878335, -12303214),
                },
                Tree {
                    site: 248,
                    container_idx: 255,
                    action: TreeAction::Delete,
                    target: (18446744070559928900, 522495),
                    parent: (18377512267673111050, -12303105),
                },
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Delete,
                    target: (723401728380764167, 168430090),
                    parent: (723401728380766730, 168430090),
                },
                Tree {
                    site: 187,
                    container_idx: 187,
                    action: TreeAction::Move,
                    target: (18444773745617749060, 1145324799),
                    parent: (4282711954, 0),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (0, 0),
                    parent: (0, 0),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (0, 0),
                    parent: (0, 0),
                },
                Tree {
                    site: 233,
                    container_idx: 59,
                    action: TreeAction::Delete,
                    target: (65283536467177, 741113388),
                    parent: (71841261820051500, 960183099),
                },
                Tree {
                    site: 59,
                    container_idx: 52,
                    action: TreeAction::Move,
                    target: (6557365790174437622, 56576),
                    parent: (8174483510898259552, 993606491),
                },
                Tree {
                    site: 52,
                    container_idx: 59,
                    action: TreeAction::Create,
                    target: (4268070197412969276, 876297019),
                    parent: (4251678678166733311, 993606459),
                },
                Tree {
                    site: 52,
                    container_idx: 59,
                    action: TreeAction::Delete,
                    target: (25614710117868896, 1610612957),
                    parent: (6589172633665888502, 1057021184),
                },
                Tree {
                    site: 59,
                    container_idx: 59,
                    action: TreeAction::Create,
                    target: (16855260271260416827, 993782249),
                    parent: (6918092204857223657, 738197563),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (23551, 0),
                    parent: (4919218170291346652, -1),
                },
            ],
        )
    }

    #[test]
    fn tree3() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 9,
                    container_idx: 127,
                    action: TreeAction::Delete,
                    target: (1233705222371475711, -251),
                    parent: (107815795232281087, 1245200),
                },
                Tree {
                    site: 16,
                    container_idx: 0,
                    action: TreeAction::Delete,
                    target: (2166517298094931967, 16716287),
                    parent: (813756299728325376, 185273099),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (8830553686128, 2097151761),
                    parent: (11958314308755580, 2137012736),
                },
                SyncAll,
                Tree {
                    site: 224,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (7306353058599232869, 6645093),
                    parent: (1224979099300004095, -2130771968),
                },
                Tree {
                    site: 13,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (9114861775483852318, 2122219134),
                    parent: (18446744073701064318, -144163841),
                },
                Sync { from: 0, to: 0 },
                Tree {
                    site: 69,
                    container_idx: 69,
                    action: TreeAction::Create,
                    target: (4991471925827290437, 4539717),
                    parent: (0, 1162167552),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (9079850878260722432, 2122219134),
                    parent: (9114861775500508798, 645824126),
                },
                Tree {
                    site: 176,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (17942534503578730496, 1),
                    parent: (16710579122929139712, 8410879),
                },
                SyncAll,
                Tree {
                    site: 126,
                    container_idx: 126,
                    action: TreeAction::Move,
                    target: (8970326032777871486, 100957280),
                    parent: (2959208222675708540, -2088042497),
                },
                Tree {
                    site: 8,
                    container_idx: 0,
                    action: TreeAction::Delete,
                    target: (3026423894714745087, -419037185),
                    parent: (257836936325095, 0),
                },
                Tree {
                    site: 37,
                    container_idx: 37,
                    action: TreeAction::Move,
                    target: (1217102201655164929, 250930404),
                    parent: (8795666948567273728, 159021690),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (9114861777589509188, 25067134),
                    parent: (2773793502260002430, 838827776),
                },
                Tree {
                    site: 4,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (8472539153, 88080384),
                    parent: (4919152350102808365, 1146307652),
                },
                SyncAll,
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (658973761946854694, 8388991),
                    parent: (17646480001140523265, 25614),
                },
                Tree {
                    site: 37,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (0, 623191711),
                    parent: (8945597075685385509, 6645116),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (795741901218843403, 185273099),
                    parent: (3108366801636107, 0),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (795741901218843403, 185273099),
                    parent: (795741901218843403, 185273099),
                },
                Tree {
                    site: 11,
                    container_idx: 11,
                    action: TreeAction::Create,
                    target: (184549376, 0),
                    parent: (18401144032249643007, 285220863),
                },
                SyncAll,
                Tree {
                    site: 37,
                    container_idx: 37,
                    action: TreeAction::Create,
                    target: (1953184550209191936, 185273115),
                    parent: (795741901218843403, 185273099),
                },
            ],
        )
    }

    #[test]
    fn tree2() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 49,
                    container_idx: 55,
                    action: TreeAction::Create,
                    target: (10546688557951311123, 786434),
                    parent: (18446462598733955072, -1785377183),
                },
                Tree {
                    site: 0,
                    container_idx: 97,
                    action: TreeAction::Create,
                    target: (10561665232893847867, 1263506066),
                    parent: (8020716663421751093, -177),
                },
                SyncAll,
                Tree {
                    site: 96,
                    container_idx: 231,
                    action: TreeAction::Create,
                    target: (2089831853518198222, -2105376152),
                    parent: (9404222468949967490, -2105376126),
                },
                Sync { from: 130, to: 104 },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (10778747784203862503, -1600809650),
                    parent: (278245130829824, -1790025217),
                },
                Tree {
                    site: 47,
                    container_idx: 254,
                    action: TreeAction::Create,
                    target: (6422527, 201327104),
                    parent: (18691781558272, 1644166912),
                },
                Tree {
                    site: 149,
                    container_idx: 97,
                    action: TreeAction::Move,
                    target: (18446468096290914321, 65535),
                    parent: (11869226595745632, 10381312),
                },
            ],
        )
    }

    #[test]
    fn tree5() {
        test_multi_sites(
            5,
            &mut vec![
                Tree {
                    site: 14,
                    container_idx: 191,
                    action: TreeAction::Delete,
                    target: (414464409599, 255),
                    parent: (4968596288896434432, 1145324612),
                },
                SyncAll,
                Tree {
                    site: 17,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (34359738623, 1029),
                    parent: (64738145635133189, 1751653634),
                },
                SyncAll,
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (0, -1583284224),
                    parent: (369057, -1583284224),
                },
                SyncAll,
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (35322350018592125, 3817472),
                    parent: (71870677115347968, 16809472),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (11646767825262886462, 10592673),
                    parent: (9042521604759584125, 3072),
                },
                Tree {
                    site: 0,
                    container_idx: 7,
                    action: TreeAction::Create,
                    target: (11618792525711540224, -1583242847),
                    parent: (303521, -1583284224),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (123146887330169, 0),
                    parent: (9042521065783296000, 2105377409),
                },
                Sync { from: 125, to: 125 },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (6801176102849887806, -10552482),
                    parent: (11646590111356878847, 4104609),
                },
                SyncAll,
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (9042527119497592064, 1073741949),
                    parent: (71870926773289018, 33390080),
                },
                SyncAll,
                Tree {
                    site: 62,
                    container_idx: 62,
                    action: TreeAction::Move,
                    target: (9042384321031283105, 2105376125),
                    parent: (201358717, 1040187392),
                },
                SyncAll,
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (8719105218031124542, 1585019257),
                    parent: (28672, 0),
                },
                SyncAll,
                Tree {
                    site: 0,
                    container_idx: 64,
                    action: TreeAction::Create,
                    target: (143409301627557434, -452984704),
                    parent: (8863198864297755136, 6842472),
                },
                Sync { from: 161, to: 62 },
                Tree {
                    site: 124,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (7340126, 0),
                    parent: (9042521604759552000, 32125),
                },
                Tree {
                    site: 36,
                    container_idx: 58,
                    action: TreeAction::Delete,
                    target: (26392582291456, 0),
                    parent: (11618792525715619574, -1583242847),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (268999696801, 2030074880),
                    parent: (123146887330169, 0),
                },
                Tree {
                    site: 0,
                    container_idx: 125,
                    action: TreeAction::Move,
                    target: (16395917401619837, 1446650880),
                    parent: (36030985271247103, 33613056),
                },
                Sync { from: 0, to: 123 },
                Sync { from: 106, to: 0 },
                Tree {
                    site: 77,
                    container_idx: 77,
                    action: TreeAction::Create,
                    target: (4683480280727375181, 1573120),
                    parent: (4485022278907592704, 1044266558),
                },
                Sync { from: 161, to: 161 },
                Sync { from: 161, to: 62 },
                Tree {
                    site: 152,
                    container_idx: 152,
                    action: TreeAction::Move,
                    target: (11000209871015024792, 26780056),
                    parent: (1177972780094232704, 36608),
                },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (2233785415186356376, 522133279),
                    parent: (16326094112104223, -1734240104),
                },
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (16493559523500359679, -454761244),
                    parent: (16493559407081481444, 15000804),
                },
                SyncAll,
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Delete,
                    target: (8759942804610847, 1077952642),
                    parent: (4629771061636907072, 1077952576),
                },
                Tree {
                    site: 1,
                    container_idx: 35,
                    action: TreeAction::Create,
                    target: (4629771061636890624, 1077952576),
                    parent: (2667528256, 16711680),
                },
                SyncAll,
                Tree {
                    site: 78,
                    container_idx: 78,
                    action: TreeAction::Move,
                    target: (15924828756454277119, -161480704),
                    parent: (949193765217, 16777209),
                },
            ],
        )
    }

    #[test]
    fn tree_replace_old_parent() {
        test_multi_sites(
            5,
            &mut vec![
                Tree {
                    site: 0,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (10127624197329226892, -1936946036),
                    parent: (10127624197330734220, -1936946036),
                },
                Tree {
                    site: 142,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (10127624197330734220, -1936946036),
                    parent: (10127624197330734220, -1946125807),
                },
                Tree {
                    site: 140,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (1420, -1936946176),
                    parent: (10127624197330734220, -1936946036),
                },
                Tree {
                    site: 140,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (10127623737769233548, -1936946036),
                    parent: (10127624197330734220, -1936962164),
                },
                Tree {
                    site: 140,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (10127624197330734220, -1938453364),
                    parent: (10127624197330734220, -1936946036),
                },
                SyncAll,
                SyncAll,
                Tree {
                    site: 140,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (5, -1936946036),
                    parent: (10127624197330734220, -1936946036),
                },
                Tree {
                    site: 140,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (10127624195535572108, -1936946036),
                    parent: (10127624197330734220, 864848973),
                },
                Tree {
                    site: 140,
                    container_idx: 140,
                    action: TreeAction::Move,
                    target: (10127624197330734220, -1936946036),
                    parent: (10127624197330734220, -1936946036),
                },
                Tree {
                    site: 140,
                    container_idx: 17,
                    action: TreeAction::Move,
                    target: (10127598908563295372, -1936946036),
                    parent: (10127624197330734220, -1936946036),
                },
                Tree {
                    site: 140,
                    container_idx: 214,
                    action: TreeAction::Move,
                    target: (10127624197330734732, -1936946036),
                    parent: (10127624197330734220, -1936946036),
                },
                Tree {
                    site: 128,
                    container_idx: 128,
                    action: TreeAction::Move,
                    target: (10127610951449477248, -1936946036),
                    parent: (10127624197330734220, -2716539),
                },
            ],
        )
    }

    #[test]
    fn tree_meta2() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 68,
                    container_idx: 68,
                    action: TreeAction::Move,
                    target: (4971973958552256511, 1157579844),
                    parent: (1663823979171038354, 387389207),
                },
                Tree {
                    site: 23,
                    container_idx: 23,
                    action: TreeAction::Create,
                    target: (1663823975275763479, 1513239),
                    parent: (18446744069802491904, -1157625864),
                },
                Tree {
                    site: 68,
                    container_idx: 255,
                    action: TreeAction::Meta,
                    target: (17457358724263116799, -12257212),
                    parent: (4941210755937475839, -458940),
                },
            ],
        )
    }

    #[test]
    fn tree_meta3() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 83,
                    container_idx: 68,
                    action: TreeAction::Delete,
                    target: (6144232899428974267, -12303292),
                    parent: (64457769666740223, 1136376803),
                },
                Tree {
                    site: 83,
                    container_idx: 126,
                    action: TreeAction::Create,
                    target: (4485090715960753726, 1145328467),
                    parent: (144106391970530482, -134021120),
                },
                SyncAll,
                SyncAll,
                Tree {
                    site: 83,
                    container_idx: 198,
                    action: TreeAction::Delete,
                    target: (1374463284756593595, 320017171),
                    parent: (1374463283923456787, 320017171),
                },
                Tree {
                    site: 19,
                    container_idx: 19,
                    action: TreeAction::Create,
                    target: (1374463286960132883, 320017171),
                    parent: (1374463283923456787, 320017171),
                },
                Tree {
                    site: 19,
                    container_idx: 19,
                    action: TreeAction::Create,
                    target: (1374463902398747411, 320017171),
                    parent: (1374463283923456787, 320017171),
                },
                Tree {
                    site: 85,
                    container_idx: 68,
                    action: TreeAction::Meta,
                    target: (48946959133704191, 0),
                    parent: (4485090716314435584, 1044266558),
                },
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Move,
                    target: (5999845544699159807, 1397969747),
                    parent: (18446743267408233540, 7602687),
                },
            ],
        )
    }

    #[test]
    fn tree_meta4() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Meta,
                    target: (18446742974197989375, -1),
                    parent: (12826251736570199838, 520028164),
                },
                Tree {
                    site: 1,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (16625775453143040, 1761552105),
                    parent: (17654109439859425792, -553647873),
                },
                Tree {
                    site: 128,
                    container_idx: 125,
                    action: TreeAction::Meta,
                    target: (18446744073692774400, 1140849151),
                    parent: (4846791580151137091, -2147418307),
                },
                SyncAll,
                Tree {
                    site: 67,
                    container_idx: 67,
                    action: TreeAction::Meta,
                    target: (18446742974204248064, 150996991),
                    parent: (18446505380905958145, 1330592767),
                },
                Tree {
                    site: 17,
                    container_idx: 59,
                    action: TreeAction::Meta,
                    target: (1224980236811632639, 255),
                    parent: (18446743008557662719, 16777460),
                },
                SyncAll,
                Tree {
                    site: 104,
                    container_idx: 104,
                    action: TreeAction::Move,
                    target: (65283536480360, -1545651360),
                    parent: (18446462600351842559, 524287),
                },
                Tree {
                    site: 0,
                    container_idx: 233,
                    action: TreeAction::Meta,
                    target: (1229783205210443579, 652804155),
                    parent: (291370715578367, 65297),
                },
            ],
        )
    }

    #[test]
    fn tree_meta_container() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 146,
                    container_idx: 68,
                    action: TreeAction::Meta,
                    target: (10539624087947575836, -48060),
                    parent: (4919337068460520959, 1150436607),
                },
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (4952757824032145407, 1150476543),
                    parent: (18446736377124224836, -12303292),
                },
                SyncAll,
                SyncAll,
                Tree {
                    site: 68,
                    container_idx: 68,
                    action: TreeAction::Meta,
                    target: (4941087607480665156, -188),
                    parent: (4952757824032145407, 1150476543),
                },
                Tree {
                    site: 0,
                    container_idx: 255,
                    action: TreeAction::Meta,
                    target: (2089670193885516356, 1145324546),
                    parent: (18446743267406136388, -513537),
                },
                SyncAll,
                Tree {
                    site: 255,
                    container_idx: 255,
                    action: TreeAction::Meta,
                    target: (10583739794993119239, 1040139332),
                    parent: (4919132559297282047, -1840971777),
                },
                Tree {
                    site: 187,
                    container_idx: 187,
                    action: TreeAction::Delete,
                    target: (18444773745600971844, 1145324799),
                    parent: (4919217516204851199, 486539256),
                },
                Tree {
                    site: 68,
                    container_idx: 41,
                    action: TreeAction::Move,
                    target: (18446537660035301188, -117440513),
                    parent: (10583739794993119239, -48060),
                },
                SyncAll,
                Tree {
                    site: 68,
                    container_idx: 248,
                    action: TreeAction::Move,
                    target: (15481123706782866, 1157562368),
                    parent: (18446537660035301188, -117440513),
                },
            ],
        )
    }

    #[test]
    fn tree_0() {
        test_multi_sites(
            5,
            &mut [
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (6148914691236517205, -43691),
                    parent: (6156420687763341311, 1431655765),
                },
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (6148914691085522261, 1431655765),
                    parent: (6148914691236517205, 1431655765),
                },
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (6148914691236517205, 1431655765),
                    parent: (6148914691236517205, 1090475349),
                },
                Tree {
                    site: 122,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (4485090715960753726, 943208504),
                    parent: (4051049678932293688, 943208504),
                },
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (6151166491050202453, 1431655765),
                    parent: (18295140478440789, 1195839745),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (5136152271503443783, 122111815),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (5128677179139770183, 1195853639),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (5136152271503427399, 1195853639),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (5136152271503443783, 1195853639),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (5136152271503443783, 1195853639),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 182,
                    container_idx: 184,
                    action: TreeAction::Create,
                    target: (5497853135693813784, 1195854924),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (668581441151911751, 0),
                    parent: (5136152271498772480, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (5136152271503443783, 1195853639),
                    parent: (5136152271503443783, 1195853639),
                },
                Tree {
                    site: 71,
                    container_idx: 71,
                    action: TreeAction::Move,
                    target: (13310589115948287815, 404232236),
                    parent: (5497853135693827096, 1280068684),
                },
                Tree {
                    site: 24,
                    container_idx: 255,
                    action: TreeAction::Create,
                    target: (7016996347047838720, 895574369),
                    parent: (9223936088976472370, 65385),
                },
                Tree {
                    site: 0,
                    container_idx: 1,
                    action: TreeAction::Create,
                    target: (3761688987579973632, 3421236),
                    parent: (72269528138039296, -16384477),
                },
                Tree {
                    site: 76,
                    container_idx: 76,
                    action: TreeAction::Create,
                    target: (2748795787288, 4),
                    parent: (3617904946535555425, 10377529),
                },
                Sync { from: 255, to: 0 },
                Tree {
                    site: 0,
                    container_idx: 0,
                    action: TreeAction::Move,
                    target: (4485090467895050240, 1044266558),
                    parent: (4051049678932293694, 943208504),
                },
                Tree {
                    site: 56,
                    container_idx: 56,
                    action: TreeAction::Create,
                    target: (4051049678932293688, 943208504),
                    parent: (4051049679033350200, 943208504),
                },
                Tree {
                    site: 56,
                    container_idx: 56,
                    action: TreeAction::Move,
                    target: (6148914691236517205, 1431655765),
                    parent: (6148914691236517205, 1431655765),
                },
                Tree {
                    site: 1,
                    container_idx: 17,
                    action: TreeAction::Move,
                    target: (4485090715960738940, 943210046),
                    parent: (6124895493227558968, 1090475349),
                },
                Tree {
                    site: 122,
                    container_idx: 0,
                    action: TreeAction::Create,
                    target: (4485090715960753726, 943208504),
                    parent: (4051049678932293688, 943208504),
                },
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (6148914691236517205, 1431655765),
                    parent: (6148914324732641365, 9257215),
                },
                Tree {
                    site: 0,
                    container_idx: 96,
                    action: TreeAction::Create,
                    target: (4051056301872922174, 943208504),
                    parent: (4051049678932293688, 1431655736),
                },
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (6148914691236517205, 138477397),
                    parent: (323238826388099329, 1044266558),
                },
                Tree {
                    site: 96,
                    container_idx: 31,
                    action: TreeAction::Create,
                    target: (18085043209519168007, -84215046),
                    parent: (18085043209503048698, 1090592251),
                },
                Tree {
                    site: 1,
                    container_idx: 1,
                    action: TreeAction::Create,
                    target: (4051049678928675073, 943208504),
                    parent: (4050486728978872376, 1431647799),
                },
                Tree {
                    site: 85,
                    container_idx: 85,
                    action: TreeAction::Move,
                    target: (4051049678932301141, 943208504),
                    parent: (4051049678932293688, 943208504),
                },
                Tree {
                    site: 56,
                    container_idx: 56,
                    action: TreeAction::Create,
                    target: (4051049678932293688, 943208504),
                    parent: (87882006846257208, 16843009),
                },
            ],
        )
    }

    #[test]
    fn to_minify() {
        minify_error(5, vec![], test_multi_sites, normalize)
    }

    #[ctor::ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
