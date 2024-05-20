use std::{
    fmt::{Debug, Formatter},
    sync::{Arc, Mutex},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro::{
    Container, ContainerID, ContainerType, Frontiers, LoroDoc, LoroValue, PeerID, UndoManager, ID,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use tracing::info_span;

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
            info_span!("ApplyDiff", id = id).in_scope(|| {
                let mut tracker = cb_tracker.lock().unwrap();
                tracker.apply_diff(e)
            })
        }));
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
            self.add_new_container(idx);
        }
    }

    pub fn undo(&mut self, undo_length: u32) {
        self.loro.attach();
        let mut before_undo = self.loro.get_deep_value();
        for _ in 0..undo_length {
            self.undo_manager.undo.undo(&self.loro).unwrap();
        }

        println!("start redo");

        for _ in 0..undo_length {
            self.undo_manager.undo.redo(&self.loro).unwrap();
        }
        let mut after_undo = self.loro.get_deep_value();
        Self::patch_tree_undo_position(&mut before_undo);
        Self::patch_tree_undo_position(&mut after_undo);
        assert_value_eq(&before_undo, &after_undo);
    }

    fn patch_tree_undo_position(a: &mut LoroValue) {
        let root = Arc::make_mut(a.as_map_mut().unwrap());
        let tree = root.get_mut("tree").unwrap();
        let nodes = Arc::make_mut(tree.as_list_mut().unwrap());
        for node in nodes.iter_mut() {
            let node = Arc::make_mut(node.as_map_mut().unwrap());
            node.remove("position");
        }
    }

    pub fn check_tracker(&self) {
        let loro = &self.loro;
        info_span!("Check tracker", "peer = {}", loro.peer_id()).in_scope(|| {
            let tracker = self.tracker.lock().unwrap();
            let loro_value = loro.get_deep_value();
            let tracker_value = tracker.to_value();
            assert_value_eq(&loro_value, &tracker_value);
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
        for (f, v) in self.history.iter() {
            let f = Frontiers::from(f);
            let from = &self.loro.state_frontiers();
            let to = &f;
            tracing::info_span!("Checkout", ?from, ?to).in_scope(|| {
                self.loro.checkout(&f).unwrap();
                // self.loro.check_state_correctness_slow();
                let actual = self.loro.get_deep_value();
                assert_value_eq(v, &actual);
            });
        }
        let f = self.rand_frontiers();
        if f.is_empty() {
            return;
        }

        self.loro.checkout(&f).unwrap();
        self.loro.check_state_correctness_slow();
        // check snapshot correctness after checkout
        self.loro.checkout_to_latest();
        let new_doc = LoroDoc::new();
        new_doc.import(&self.loro.export_snapshot()).unwrap();
        new_doc.checkout(&f).unwrap();
        new_doc.check_state_correctness_slow();
    }

    fn rand_frontiers(&mut self) -> Frontiers {
        let vv = self.loro.oplog_vv();
        let frontiers_num = self.rng.gen_range(1..5);
        let mut frontiers: Frontiers = Frontiers::default();

        if vv.len() == 0 {
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
        let mut ids: Vec<ID> = f.iter().cloned().collect();
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

pub fn assert_value_eq(a: &LoroValue, b: &LoroValue) {
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

                    if !eq(v, b.get(k).unwrap_or(&LoroValue::I64(0))) {
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

                    if !eq(v, a.get(k).unwrap_or(&LoroValue::I64(0))) {
                        return false;
                    }
                }

                true
            }
            (a, b) => a == b,
        }
    }
    assert!(
        eq(a, b),
        "Expect left == right, but\nleft = {:#?}\nright = {:#?}",
        a,
        b
    );
}
