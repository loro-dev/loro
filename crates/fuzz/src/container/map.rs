use std::sync::{Arc, Mutex};

use loro::{Container, ContainerID, ContainerType, LoroDoc, LoroMap, LoroValue};

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{ActionExecutor, ActorTrait},
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
        );

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
        self.containers.push(container.into_map().unwrap());
    }

    fn check_tracker(&self) {
        let map = self.loro.get_map("map");
        let value_a = map.get_deep_value();
        let value_b = self.tracker.lock().unwrap().to_value();
        assert_eq!(&value_a, value_b.into_map().unwrap().get("map").unwrap());
    }

    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }
}

#[derive(Debug, Clone)]
pub enum MapAction {
    Insert { key: u8, value: FuzzValue },
    Delete { key: u8 },
}

impl MapAction {
    fn key(&self) -> u8 {
        match self {
            MapAction::Insert { key, .. } => *key,
            MapAction::Delete { key, .. } => *key,
        }
    }

    fn value_string(&self) -> String {
        match self {
            MapAction::Insert { value, .. } => value.to_string(),
            MapAction::Delete { .. } => "null".to_string(),
        }
    }
}

impl FromGenericAction for MapAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        match action.bool {
            true => MapAction::Insert {
                key: (action.key % 256) as u8,
                value: action.value,
            },
            false => MapAction::Delete {
                key: (action.key % 256) as u8,
            },
        }
    }
}

impl Actionable for MapAction {
    fn pre_process(&mut self, _actor: &mut ActionExecutor, _c: usize) {}

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let actor = actor.as_map_actor_mut().unwrap();
        let handler = actor.get_create_container_mut(container);
        match self {
            MapAction::Insert { key, value, .. } => {
                let key = &key.to_string();
                match value {
                    FuzzValue::I32(v) => {
                        handler.insert(key, LoroValue::from(*v)).unwrap();
                        None
                    }
                    FuzzValue::Container(c) => {
                        let container = handler.insert_container(key, Container::new(*c)).unwrap();
                        Some(container)
                    }
                }
            }
            MapAction::Delete { key, .. } => {
                handler.delete(&key.to_string()).unwrap();
                None
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
