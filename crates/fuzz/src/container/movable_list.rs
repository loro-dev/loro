use std::sync::{Arc, Mutex};

use loro::{Container, ContainerID, ContainerType, LoroDoc, LoroMovableList};

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{ActionExecutor, ActorTrait},
    crdt_fuzzer::FuzzValue,
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

#[derive(Debug, Clone)]
pub enum MovableListAction {
    Insert { pos: u8, value: FuzzValue },
    Delete { pos: u8, len: u8 },
    Move { from: u8, to: u8 },
    Set { pos: u8, value: FuzzValue },
}

pub struct MovableListActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroMovableList>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl MovableListActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty(ContainerID::new_root("sys:root", ContainerType::Map));
        tracker.insert(
            "movable_list".to_string(),
            Value::empty_container(
                ContainerType::MovableList,
                ContainerID::new_root("movable_list", ContainerType::MovableList),
            ),
        );
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(tracker)));
        let list = tracker.clone();

        loro.subscribe(
            &ContainerID::new_root("movable_list", ContainerType::MovableList),
            Arc::new(move |event| {
                let mut list = list.lock().unwrap();
                list.apply_diff(event);
            }),
        );

        let root = loro.get_movable_list("movable_list");
        Self {
            loro,
            containers: vec![root],
            tracker,
        }
    }

    pub fn get_create_container_mut(&mut self, container_idx: usize) -> &mut LoroMovableList {
        if self.containers.is_empty() {
            let handler = self.loro.get_movable_list("movable_list");
            self.containers.push(handler);
            self.containers.last_mut().unwrap()
        } else {
            self.containers.get_mut(container_idx).unwrap()
        }
    }
}

impl ActorTrait for MovableListActor {
    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }

    fn check_tracker(&self) {
        let list = self.loro.get_movable_list("movable_list");
        let value = list.get_deep_value();
        let tracker = self.tracker.lock().unwrap().to_value();
        assert_eq!(
            &value,
            tracker.into_map().unwrap().get("movable_list").unwrap()
        );
    }

    fn add_new_container(&mut self, container: Container) {
        self.containers.push(container.into_movable_list().unwrap());
    }
}

impl Actionable for MovableListAction {
    fn pre_process(&mut self, actor: &mut ActionExecutor, container: usize) {
        let actor = actor.as_movable_list_actor().unwrap();
        let list = actor.containers.get(container).unwrap();
        let length = list.len();
        match self {
            MovableListAction::Insert { pos, value: _ } => {
                *pos %= length.max(1) as u8;
            }
            MovableListAction::Delete { pos, len } => {
                if list.is_empty() {
                    *self = MovableListAction::Insert {
                        pos: 0,
                        value: FuzzValue::I32(*pos as i32),
                    };
                } else {
                    *pos %= length.max(1) as u8;
                    *len %= (length as u8).saturating_sub(*pos).max(1);
                }
            }
            MovableListAction::Move { from, to } => {
                if list.is_empty() {
                    *self = MovableListAction::Insert {
                        pos: 0,
                        value: FuzzValue::I32(*from as i32),
                    };
                } else {
                    *from %= length.max(1) as u8;
                    *to %= length.max(1) as u8;
                }
            }
            MovableListAction::Set { pos, value } => {
                if list.is_empty() {
                    *self = MovableListAction::Insert {
                        pos: 0,
                        value: *value,
                    };
                } else {
                    *pos %= length.max(1) as u8;
                }
            }
        }
    }

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let actor = actor.as_movable_list_actor_mut().unwrap();
        let list = actor.get_create_container_mut(container);
        match self {
            MovableListAction::Insert { pos, value } => {
                let pos = *pos as usize;
                match value {
                    FuzzValue::Container(c) => {
                        let container = list.insert_container(pos, *c).unwrap();
                        Some(container)
                    }
                    FuzzValue::I32(v) => {
                        list.insert(pos, *v).unwrap();
                        None
                    }
                }
            }
            MovableListAction::Delete { pos, len } => {
                let pos = *pos as usize;
                let len = *len as usize;
                list.delete(pos, len).unwrap();
                None
            }
            MovableListAction::Move { from, to } => {
                let from = *from as usize;
                let to = *to as usize;
                list.mov(from, to).unwrap();
                None
            }
            MovableListAction::Set { pos, value } => {
                let pos = *pos as usize;
                match value {
                    FuzzValue::Container(c) => {
                        let container = list.set_container(pos, *c).unwrap();
                        Some(container)
                    }
                    FuzzValue::I32(v) => {
                        list.set(pos, *v).unwrap();
                        None
                    }
                }
            }
        }
    }

    fn ty(&self) -> ContainerType {
        ContainerType::MovableList
    }

    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2] {
        match self {
            MovableListAction::Insert { pos, value } => {
                [format!("insert {}", pos).into(), value.to_string().into()]
            }
            MovableListAction::Delete { pos, len } => {
                ["delete".into(), format!("{} ~ {}", pos, pos + len).into()]
            }
            MovableListAction::Move { from, to } => {
                ["move".into(), format!("{} -> {}", from, to).into()]
            }
            MovableListAction::Set { pos, value } => {
                [format!("set {}", pos).into(), value.to_string().into()]
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "MovableList"
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        match self {
            MovableListAction::Insert { value, .. } => match value {
                FuzzValue::Container(c) => Some(c),
                _ => None,
            },
            MovableListAction::Delete { .. } => None,
            MovableListAction::Move { .. } => None,
            MovableListAction::Set { value, .. } => match value {
                FuzzValue::Container(c) => Some(c),
                _ => None,
            },
        }
    }
}

impl FromGenericAction for MovableListAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        match action.prop % 4 {
            0 => MovableListAction::Insert {
                pos: (action.pos % 256) as u8,
                value: action.value,
            },
            1 => MovableListAction::Delete {
                pos: (action.pos % 256) as u8,
                len: (action.length % 256) as u8,
            },
            2 => MovableListAction::Move {
                from: (action.pos % 256) as u8,
                to: (action.length % 256) as u8,
            },
            3 => MovableListAction::Set {
                pos: (action.pos % 256) as u8,
                value: action.value,
            },
            _ => unreachable!(),
        }
    }
}
