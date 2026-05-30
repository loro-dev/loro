use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

#[cfg(feature = "mergeable")]
use loro::LoroError;
use loro::{Container, ContainerID, ContainerTrait, ContainerType, LoroDoc, LoroMap, LoroValue};

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{assert_value_eq, ActionExecutor, ActorTrait},
    crdt_fuzzer::FuzzValue,
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

pub struct MapActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroMap>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl MapActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty(ContainerID::new_root("sys:root", ContainerType::Map));
        tracker.insert(
            "map".to_string(),
            Value::empty_container(
                ContainerType::Map,
                ContainerID::new_root("map", ContainerType::Map),
            ),
        );
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(tracker)));
        let map = tracker.clone();
        loro.subscribe(
            &ContainerID::new_root("map", ContainerType::Map),
            Arc::new(move |event| {
                let mut map = map.lock().unwrap();
                map.apply_diff(event);
            }),
        )
        .detach();

        let root = loro.get_map("map");
        MapActor {
            loro,
            containers: vec![root],
            tracker,
        }
    }

    pub fn get_create_container_mut(&mut self, container_idx: usize) -> &mut LoroMap {
        if self.containers.is_empty() {
            let handler = self.loro.get_map("map");
            self.containers.push(handler);
            self.containers.last_mut().unwrap()
        } else {
            self.containers.get_mut(container_idx).unwrap()
        }
    }
}

impl ActorTrait for MapActor {
    fn add_new_container(&mut self, container: Container) {
        // Dedup by cid so mergeable children don't bias the dispatch index.
        let cid = container.id();
        if self.containers.iter().any(|c| c.id() == cid) {
            return;
        }
        self.containers.push(container.into_map().unwrap());
    }

    fn check_tracker(&self) {
        let map = self.loro.get_map("map");
        let value_a = map.get_deep_value();
        let value_b = self.tracker.lock().unwrap().to_value();
        assert_value_eq(
            &value_a,
            value_b.into_map().unwrap().get("map").unwrap(),
            None,
        );
    }

    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }
}

#[derive(Clone)]
pub enum MapAction {
    Insert {
        key: u8,
        value: FuzzValue,
    },
    Delete {
        key: u8,
    },
    Clear,
    #[cfg(feature = "mergeable")]
    GetMergeable {
        key: u8,
        kind: ContainerType,
    },
}

impl Debug for MapAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapAction::Insert { key, value } => {
                write!(
                    f,
                    "MapAction::Insert {{ key: {}, value: {:?} }}",
                    key, value
                )
            }
            MapAction::Delete { key } => write!(f, "MapAction::Delete {{ key: {} }}", key),
            MapAction::Clear => write!(f, "MapAction::Clear"),
            #[cfg(feature = "mergeable")]
            MapAction::GetMergeable { key, kind } => write!(
                f,
                "MapAction::GetMergeable {{ key: {}, kind: {:?} }}",
                key, kind
            ),
        }
    }
}

impl MapAction {
    fn key(&self) -> u8 {
        match self {
            MapAction::Insert { key, .. } => *key,
            MapAction::Delete { key, .. } => *key,
            MapAction::Clear => 0,
            #[cfg(feature = "mergeable")]
            MapAction::GetMergeable { key, .. } => *key,
        }
    }

    fn value_string(&self) -> String {
        match self {
            MapAction::Insert { value, .. } => value.to_string(),
            MapAction::Delete { .. } => "null".to_string(),
            MapAction::Clear => "null".to_string(),
            #[cfg(feature = "mergeable")]
            MapAction::GetMergeable { kind, .. } => format!("mergeable {:?}", kind),
        }
    }
}

impl FromGenericAction for MapAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        #[cfg(not(feature = "mergeable"))]
        let modulus = 3;
        #[cfg(feature = "mergeable")]
        let modulus = 4;
        match action.prop % modulus {
            0 => MapAction::Insert {
                key: (action.key % 256) as u8,
                value: action.value,
            },
            1 => MapAction::Delete {
                key: (action.key % 256) as u8,
            },
            2 => MapAction::Clear,
            #[cfg(feature = "mergeable")]
            3 => MapAction::GetMergeable {
                key: (action.key % 256) as u8,
                kind: match action.value {
                    FuzzValue::Container(c) => c,
                    FuzzValue::I32(_) => ContainerType::Map,
                },
            },
            _ => unreachable!(),
        }
    }
}

impl Actionable for MapAction {
    fn pre_process(&mut self, _actor: &mut ActionExecutor, _c: usize) {}

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let actor = actor.as_map_actor_mut().unwrap();
        let handler = actor.get_create_container_mut(container);
        use super::unwrap;
        match self {
            MapAction::Insert { key, value, .. } => {
                let key = &key.to_string();
                match value {
                    FuzzValue::I32(v) => {
                        unwrap(handler.insert(key, LoroValue::from(*v)));
                        None
                    }
                    FuzzValue::Container(c) => {
                        unwrap(handler.insert_container(key, Container::new(*c)))
                    }
                }
            }
            MapAction::Delete { key, .. } => {
                unwrap(handler.delete(&key.to_string()));
                None
            }
            MapAction::Clear => {
                unwrap(handler.clear());
                None
            }
            #[cfg(feature = "mergeable")]
            MapAction::GetMergeable { key, kind } => {
                let key = &key.to_string();
                // Type-conflict rejection (`ArgErr`) is an expected outcome — swallow it
                // here rather than via `unwrap` so unrelated `ArgErr` sources still panic.
                let result = match kind {
                    ContainerType::Map => handler.get_mergeable_map(key).map(|m| m.to_container()),
                    ContainerType::List => {
                        handler.get_mergeable_list(key).map(|l| l.to_container())
                    }
                    ContainerType::MovableList => handler
                        .get_mergeable_movable_list(key)
                        .map(|l| l.to_container()),
                    ContainerType::Text => {
                        handler.get_mergeable_text(key).map(|t| t.to_container())
                    }
                    ContainerType::Tree => {
                        handler.get_mergeable_tree(key).map(|t| t.to_container())
                    }
                    ContainerType::Counter => {
                        handler.get_mergeable_counter(key).map(|c| c.to_container())
                    }
                    ContainerType::Unknown(_) => return None,
                };
                match result {
                    Ok(c) => Some(c),
                    Err(LoroError::ArgErr(_)) => None,
                    Err(e) => panic!("Error: {}", e),
                }
            }
        }
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        match self {
            MapAction::Insert { value, .. } => match value {
                FuzzValue::Container(c) => Some(c),
                _ => None,
            },
            MapAction::Delete { .. } => None,
            MapAction::Clear => None,
            #[cfg(feature = "mergeable")]
            MapAction::GetMergeable { kind, .. } => Some(kind),
        }
    }

    fn ty(&self) -> ContainerType {
        ContainerType::Map
    }

    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2] {
        [self.key().to_string().into(), self.value_string().into()]
    }

    fn type_name(&self) -> &'static str {
        "Map"
    }
}
