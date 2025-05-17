use std::{
    collections::VecDeque,
    fmt::{Debug, Formatter},
    sync::{Arc, Mutex},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro::{
    Container, ContainerID, ContainerType, Frontiers, LoroDoc, LoroError, LoroValue, PeerID,
    UndoManager, ID,
};
use pretty_assertions::assert_eq;
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing::{info, info_span};

use crate::{
    container::{CounterActor, ListActor, MovableListActor, TextActor, TreeActor},
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

use super::{
    actions::{ActionInner, Actionable},
    container::MapActor,
};

#[derive(Debug)]
pub struct Undo {
    pub undo: UndoManager,
    pub last_container: u8,
    pub can_undo_length: u8,
}

pub struct Actor {
    pub peer: PeerID,
    pub loro: Arc<LoroDoc>,
    pub targets: FxHashMap<ContainerType, ActionExecutor>,
    pub tracker: Arc<Mutex<ContainerTracker>>,
    pub history: FxHashMap<Vec<ID>, LoroValue>,
    pub undo_manager: Undo,
    pub rng: StdRng,
}

impl Actor {
    pub fn new(id: PeerID) -> Self {
        let loro = LoroDoc::new();
        loro.set_peer_id(id).unwrap();
        let undo = UndoManager::new(&loro);
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(MapTracker::empty(
            ContainerID::new_root("sys:root", ContainerType::Map),
        ))));
        let cb_tracker = tracker.clone();
        loro.subscribe_root(Arc::new(move |e| {
            info_span!("[Fuzz] tracker.apply_diff", id = id).in_scope(|| {
                let mut tracker = cb_tracker.lock().unwrap();
                tracker.apply_diff(e)
            });
        }))
        .detach();
        let mut default_history = FxHashMap::default();
        default_history.insert(Vec::new(), loro.get_deep_value());
        Actor {
            peer: id,
            loro: Arc::new(loro),
            tracker,
            targets: FxHashMap::default(),
            history: default_history,
            undo_manager: Undo {
                undo,
                last_container: 255,
                can_undo_length: 0,
            },
            rng: StdRng::from_seed({
                let mut seed = [0u8; 32];
                let bytes = id.to_be_bytes(); // Convert u64 to [u8; 8]
                seed[..8].copy_from_slice(&bytes); // Copy the 8 bytes into the start of the seed array
                seed
            }),
        }
    }

    pub fn add_new_container(&mut self, container: Container) {
        let actor = self.targets.get_mut(&container.get_type()).unwrap();
        match actor {
            ActionExecutor::MapActor(actor) => actor.add_new_container(container),
            ActionExecutor::ListActor(actor) => actor.add_new_container(container),
            ActionExecutor::TextActor(actor) => actor.add_new_container(container),
            ActionExecutor::TreeActor(actor) => actor.add_new_container(container),
            ActionExecutor::MovableListActor(actor) => actor.add_new_container(container),
            ActionExecutor::CounterActor(actor) => actor.add_new_container(container),
        }
    }

    pub fn pre_process(&mut self, action: &mut ActionInner, container: &mut u8) {
        let ty = action.ty();
        let mut targets = self.targets.keys().copied().collect::<Vec<_>>();
        targets.sort();
        if let Some(add_container_ty) = action.pre_process_container_value() {
            if !targets.contains(add_container_ty) {
                *add_container_ty =
                    targets.remove(add_container_ty.to_u8() as usize % targets.len());
            }
        }
        let actor = self.targets.get_mut(&ty).unwrap();
        // maybe txn is used in pre_process
        self.loro.attach();
        *container %= actor.container_len().max(1);
        action.pre_process(actor, *container as usize);
    }

    pub fn apply(&mut self, action: &ActionInner, container: u8) {
        let ty = action.ty();
        let actor = self.targets.get_mut(&ty).unwrap();
        self.loro.attach();
        let idx = action.apply(actor, container as usize);

        if self.undo_manager.last_container != container {
            self.undo_manager.last_container = container;
            self.undo_manager.can_undo_length += 1;
        }

        if let Some(idx) = idx {
            if let Container::Tree(tree) = &idx {
                tree.enable_fractional_index(0);
            }
            self.add_new_container(idx);
        }
    }

    pub fn test_undo(&mut self, undo_length: u32) {
        if !self.undo_manager.undo.can_undo() {
            return;
        }

        self.loro.attach();
        let before_undo = self.loro.get_deep_value();

        // trace!("BeforeUndo {:#?}", self.loro.get_deep_value_with_id());
        // println!("\n\nstart undo\n");
        for _ in 0..undo_length {
            self.undo_manager.undo.undo().unwrap();
            self.loro.commit();
        }
        // trace!("AfterUndo {:#?}", self.loro.get_deep_value_with_id());

        // println!("\n\nstart redo\n");
        for _ in 0..undo_length {
            self.undo_manager.undo.redo().unwrap();
            self.loro.commit();
        }
        // trace!("AfterRedo {:#?}", self.loro.get_deep_value_with_id());

        let after_undo = self.loro.get_deep_value();

        assert_value_eq(&before_undo, &after_undo, None);
        self.undo_manager.undo.clear();
    }

    pub fn check_tracker(&self) {
        let loro = &self.loro;
        info_span!("Check tracker", "peer = {}", loro.peer_id()).in_scope(|| {
            let tracker = self.tracker.lock().unwrap();
            let loro_value = loro.get_deep_value();
            let tracker_value = tracker.to_value();
            assert_value_eq(&loro_value, &tracker_value, None);
            self.targets.values().for_each(|t| t.check_tracker());
        });
    }

    pub fn check_eq(&self, other: &Actor) {
        let doc_a = &self.loro;
        let doc_b = &other.loro;
        let a_result = doc_a.get_deep_value();
        let b_result = doc_b.get_deep_value();
        assert_eq!(a_result, b_result);
    }

    pub fn check_history(&mut self) {
        // let v = self.loro.with_state(|s| s.get_all_container_value_flat());
        // tracing::info!("ContainerValue = {:#?}", v);
        // let json = self
        //     .loro
        //     .export_json_updates(&Default::default(), &self.loro.oplog_vv());
        // let string = serde_json::to_string_pretty(&json).unwrap();
        // tracing::info!("json = {}", string);
        self.loro.check_state_correctness_slow();
        for (f, v) in self.history.iter() {
            let f = Frontiers::from(f);
            let from = &self.loro.state_frontiers();
            let to = &f;
            let peer = self.peer;
            tracing::info_span!("FuzzCheckout", ?from, ?to, ?peer).in_scope(|| {
                match self.loro.checkout(&f) {
                    Ok(_) => {}
                    Err(LoroError::SwitchToVersionBeforeShallowRoot) => {
                        return;
                    }
                    Err(e) => {
                        panic!("{}", e);
                    }
                }
                // self.loro.check_state_correctness_slow();
                let actual = self.loro.get_deep_value();
                assert_value_eq(
                    v,
                    &actual,
                    Some(&mut || {
                        self.loro.with_oplog(|log| {
                            log.check_dag_correctness();
                        });
                        format!(
                            "loro.vv = {:#?}, loro updates = {:#?}",
                            self.loro.oplog_vv(),
                            self.loro
                                .export_json_updates(&Default::default(), &self.loro.oplog_vv())
                        )
                    }),
                );
            });
        }
        let f = self.rand_frontiers();
        if f.is_empty() {
            return;
        }

        match self.loro.checkout(&f) {
            Ok(_) => {
                // check snapshot correctness after checkout
                self.loro.check_state_correctness_slow();
                self.loro.checkout_to_latest();
                let new_doc = LoroDoc::new();
                info_span!("FuzzCheckoutCreatingNewSnapshotDoc",).in_scope(|| {
                    new_doc
                        .import(&self.loro.export(loro::ExportMode::Snapshot).unwrap())
                        .unwrap();
                    assert_eq!(new_doc.get_deep_value(), self.loro.get_deep_value());
                });
                info_span!("FuzzCheckoutOnNewSnapshotDoc",).in_scope(|| {
                    new_doc.checkout(&f).unwrap();
                    new_doc.check_state_correctness_slow();
                });
            }
            Err(LoroError::SwitchToVersionBeforeShallowRoot) => {}
            Err(e) => panic!("{}", e),
        }
    }

    fn rand_frontiers(&mut self) -> Frontiers {
        let vv = self.loro.oplog_vv();
        let frontiers_num = self.rng.gen_range(1..5);
        let mut frontiers: Frontiers = Frontiers::default();

        if vv.is_empty() {
            return frontiers;
        }

        for _ in 0..frontiers_num {
            let peer_idx = self.rng.gen_range(0..vv.len());
            let peer = *vv.keys().nth(peer_idx).unwrap();
            let Some(&end_counter) = vv.get(&peer) else {
                dbg!(peer, &vv, vv.len());
                panic!("WTF");
            };

            if end_counter == 0 {
                continue;
            }

            let counter = self.rng.gen_range(0..end_counter);
            frontiers.push(ID::new(peer, counter));
        }
        frontiers
    }

    pub fn record_history(&mut self) {
        self.loro.attach();
        let f = self.loro.oplog_frontiers();
        let value = self.loro.get_deep_value();
        let mut ids: Vec<ID> = f.iter().collect();
        ids.sort_by_key(|x| x.peer);
        self.history.insert(ids, value);
    }

    pub fn register(&mut self, target: ContainerType) {
        match target {
            ContainerType::Map => {
                self.tracker.lock().unwrap().as_map_mut().unwrap().insert(
                    "map".to_string(),
                    Value::empty_container(
                        ContainerType::Map,
                        ContainerID::new_root("map", ContainerType::Map),
                    ),
                );
                self.targets.insert(
                    target,
                    ActionExecutor::MapActor(MapActor::new(self.loro.clone())),
                );
            }
            ContainerType::List => {
                self.tracker.lock().unwrap().as_map_mut().unwrap().insert(
                    "list".to_string(),
                    Value::empty_container(
                        ContainerType::List,
                        ContainerID::new_root("list", ContainerType::List),
                    ),
                );
                self.targets.insert(
                    target,
                    ActionExecutor::ListActor(ListActor::new(self.loro.clone())),
                );
            }
            ContainerType::MovableList => {
                self.tracker.lock().unwrap().as_map_mut().unwrap().insert(
                    "movable_list".to_string(),
                    Value::empty_container(
                        ContainerType::MovableList,
                        ContainerID::new_root("movable_list", ContainerType::MovableList),
                    ),
                );
                self.targets.insert(
                    target,
                    ActionExecutor::MovableListActor(MovableListActor::new(self.loro.clone())),
                );
            }
            ContainerType::Text => {
                self.tracker.lock().unwrap().as_map_mut().unwrap().insert(
                    "text".to_string(),
                    Value::empty_container(
                        ContainerType::Text,
                        ContainerID::new_root("text", ContainerType::Text),
                    ),
                );
                self.targets.insert(
                    target,
                    ActionExecutor::TextActor(TextActor::new(self.loro.clone())),
                );
            }
            ContainerType::Tree => {
                self.tracker.lock().unwrap().as_map_mut().unwrap().insert(
                    "tree".to_string(),
                    Value::empty_container(
                        ContainerType::Tree,
                        ContainerID::new_root("tree", ContainerType::Tree),
                    ),
                );
                self.targets.insert(
                    target,
                    ActionExecutor::TreeActor(TreeActor::new(self.loro.clone())),
                );
            }
            ContainerType::Counter => {
                self.tracker.lock().unwrap().as_map_mut().unwrap().insert(
                    "counter".to_string(),
                    Value::empty_container(
                        ContainerType::Counter,
                        ContainerID::new_root("counter", ContainerType::Counter),
                    ),
                );
                self.targets.insert(
                    target,
                    ActionExecutor::CounterActor(CounterActor::new(self.loro.clone())),
                );
            }
            ContainerType::Unknown(_) => unreachable!(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn log_json_history(&self) {
        let json = self
            .loro
            .export_json_updates(&Default::default(), &self.loro.oplog_vv());
        let string = serde_json::to_string_pretty(&json).unwrap();
        info!("vv={:?} json = {}", self.loro.oplog_vv(), string);
    }
}

#[enum_dispatch(ActorTrait)]
#[derive(EnumAsInner)]
pub enum ActionExecutor {
    MapActor(MapActor),
    ListActor(ListActor),
    MovableListActor(MovableListActor),
    TextActor(TextActor),
    TreeActor(TreeActor),
    CounterActor(CounterActor),
}

impl Debug for ActionExecutor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionExecutor::MapActor(_) => write!(f, "MapActor"),
            ActionExecutor::ListActor(_) => write!(f, "ListActor"),
            ActionExecutor::MovableListActor(_) => write!(f, "MovableListActor"),
            ActionExecutor::TextActor(_) => write!(f, "TextActor"),
            ActionExecutor::TreeActor(_) => write!(f, "TreeActor"),
            ActionExecutor::CounterActor(_) => write!(f, "CounterActor"),
        }
    }
}

#[enum_dispatch]
pub trait ActorTrait {
    fn container_len(&self) -> u8;
    /// check the value of root container is equal to the tracker
    fn check_tracker(&self);
    fn add_new_container(&mut self, container: Container);
}

pub fn assert_value_eq(a: &LoroValue, b: &LoroValue, mut log: Option<&mut dyn FnMut() -> String>) {
    #[must_use]
    fn eq(a: &LoroValue, b: &LoroValue) -> bool {
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

                    if !eq(v, b.get(k).unwrap_or(&LoroValue::Double(0.))) {
                        return false;
                    }
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

                    if !eq(v, a.get(k).unwrap_or(&LoroValue::Double(0.))) {
                        return false;
                    }
                }

                true
            }
            (LoroValue::List(a_list), LoroValue::List(b_list)) => {
                if is_tree_values(a_list.as_ref()) {
                    assert_tree_value_eq(a_list, b_list);
                    true
                } else {
                    a_list.iter().zip(b_list.iter()).all(|(a, b)| eq(a, b))
                }
            }
            (a, b) => a == b,
        }
    }
    assert!(
        eq(a, b),
        "Expect left == right, but\nleft = {:#?}\nright = {:#?}\n{}",
        a,
        b,
        log.as_mut().map_or(String::new(), |f| f())
    );
}

pub fn is_tree_values(value: &[LoroValue]) -> bool {
    if let Some(LoroValue::Map(map)) = value.first() {
        let map_keys = map.as_ref().keys().cloned().collect::<FxHashSet<_>>();
        return map_keys.contains("id")
            && map_keys.contains("parent")
            && map_keys.contains("meta")
            && map_keys.contains("fractional_index");
    }
    false
}
#[derive(Debug, Clone)]
struct Node {
    children: Vec<Node>,
    meta: FxHashMap<String, LoroValue>,
    position: String,
}

impl Node {
    fn from_loro_value(value: &[LoroValue]) -> Vec<Self> {
        let mut node_map = FxHashMap::default();
        let mut parent_child_map = FxHashMap::default();
        for node in value.iter() {
            let map = node.as_map().unwrap();
            let id = map.get("id").unwrap().as_string().unwrap().to_string();
            let parent = map
                .get("parent")
                .unwrap()
                .as_string()
                .map(|x| x.to_string());

            let meta = map.get("meta").unwrap().as_map().unwrap().as_ref().clone();
            let index = *map.get("index").unwrap().as_i64().unwrap() as usize;
            let position = map
                .get("fractional_index")
                .unwrap()
                .as_string()
                .unwrap()
                .to_string();
            let children = map.get("children").unwrap().as_list().unwrap();
            let children = Node::from_loro_value(children);
            let tree_node = Node {
                children,
                meta,
                position,
            };

            node_map.insert(id.clone(), tree_node);

            parent_child_map
                .entry(parent)
                .or_insert_with(Vec::new)
                .push((index, id));
        }
        let mut node_map_clone = node_map.clone();
        for (parent_id, child_ids) in parent_child_map.iter() {
            if let Some(parent_id) = parent_id {
                if let Some(parent_node) = node_map.get_mut(parent_id) {
                    for (_, child_id) in child_ids.iter().sorted_by_key(|x| x.0) {
                        if let Some(child_node) = node_map_clone.remove(child_id) {
                            parent_node.children.push(child_node);
                        }
                    }
                }
            }
        }

        parent_child_map.get(&None).map_or(vec![], |root_ids| {
            root_ids
                .iter()
                .filter_map(|(_i, id)| node_map.remove(id))
                .collect::<Vec<_>>()
        })
    }
}

pub fn assert_tree_value_eq(a: &[LoroValue], b: &[LoroValue]) {
    let a_tree = Node::from_loro_value(a);
    let b_tree = Node::from_loro_value(b);
    let mut a_q = VecDeque::from_iter([a_tree]);
    let mut b_q = VecDeque::from_iter([b_tree]);
    while let (Some(a_node), Some(b_node)) = (a_q.pop_front(), b_q.pop_front()) {
        let mut children_a = vec![];
        let mut children_b = vec![];
        let a_meta = a_node
            .into_iter()
            .map(|x| {
                children_a.extend(x.children);
                let mut meta = x
                    .meta
                    .into_iter()
                    .sorted_by_cached_key(|(k, _)| k.clone())
                    .map(|(mut k, v)| {
                        k.push_str(v.as_string().map_or("", |f| f.as_str()));
                        k
                    })
                    .collect::<String>();
                meta.push_str(&x.position);
                meta
            })
            .collect::<FxHashSet<_>>();
        let b_meta = b_node
            .into_iter()
            .map(|x| {
                children_b.extend(x.children);
                let mut meta = x
                    .meta
                    .into_iter()
                    .sorted_by_cached_key(|(k, _)| k.clone())
                    .map(|(mut k, v)| {
                        k.push_str(v.as_string().map_or("", |f| f.as_str()));
                        k
                    })
                    .collect::<String>();
                meta.push_str(&x.position);
                meta
            })
            .collect::<FxHashSet<_>>();
        assert!(a_meta.difference(&b_meta).count() == 0);
        assert_eq!(children_a.len(), children_b.len());
        if children_a.is_empty() {
            continue;
        }
        a_q.push_back(children_a);
        b_q.push_back(children_b);
    }
}
