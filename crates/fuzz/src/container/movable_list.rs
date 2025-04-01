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
        )
        .detach();

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
            Self::Insert { pos, value: _ } => {
                *pos %= length.max(1) as u8;
            }
            Self::Delete { pos, len } => {
                if list.is_empty() {
                    *self = Self::Insert {
                        pos: 0,
                        value: FuzzValue::I32(*pos as i32),
                    };
                } else {
                    *pos %= length.max(1) as u8;
                    *len %= (length as u8).saturating_sub(*pos).max(1);
                }
            }
            Self::Move { from, to } => {
                if list.is_empty() {
                    *self = Self::Insert {
                        pos: 0,
                        value: FuzzValue::I32(*from as i32),
                    };
                } else {
                    *from %= length.max(1) as u8;
                    *to %= length.max(1) as u8;
                }
            }
            Self::Set { pos, value } => {
                if list.is_empty() {
                    *self = Self::Insert {
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
            Self::Insert { pos, value } => {
                let pos = *pos as usize;
                match value {
                    FuzzValue::Container(c) => {
                        super::unwrap(list.insert_container(pos, Container::new(*c)))
                    }
                    FuzzValue::I32(v) => {
                        super::unwrap(list.insert(pos, *v));
                        None
                    }
                }
            }
            Self::Delete { pos, len } => {
                let pos = *pos as usize;
                let len = *len as usize;
                super::unwrap(list.delete(pos, len));
                None
            }
            Self::Move { from, to } => {
                let from = *from as usize;
                let to = *to as usize;
                super::unwrap(list.mov(from, to));
                None
            }
            Self::Set { pos, value } => {
                let pos = *pos as usize;
                match value {
                    FuzzValue::Container(c) => {
                        super::unwrap(list.set_container(pos, Container::new(*c)))
                    }
                    FuzzValue::I32(v) => {
                        super::unwrap(list.set(pos, *v));
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
            Self::Insert { pos, value } => {
                [format!("insert {}", pos).into(), value.to_string().into()]
            }
            Self::Delete { pos, len } => {
                ["delete".into(), format!("{} ~ {}", pos, pos + len).into()]
            }
            Self::Move { from, to } => {
                ["move".into(), format!("{} -> {}", from, to).into()]
            }
            Self::Set { pos, value } => {
                [format!("set {}", pos).into(), value.to_string().into()]
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "MovableList"
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        match self {
            Self::Insert { value, .. } => match value {
                FuzzValue::Container(c) => Some(c),
                _ => None,
            },
            Self::Delete { .. } => None,
            Self::Move { .. } => None,
            Self::Set { value, .. } => match value {
                FuzzValue::Container(c) => Some(c),
                _ => None,
            },
        }
    }
}

impl FromGenericAction for MovableListAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        match action.prop % 4 {
            0 => Self::Insert {
                pos: (action.pos % 256) as u8,
                value: action.value,
            },
            1 => Self::Delete {
                pos: (action.pos % 256) as u8,
                len: (action.length % 256) as u8,
            },
            2 => Self::Move {
                from: (action.pos % 256) as u8,
                to: (action.length % 256) as u8,
            },
            3 => Self::Set {
                pos: (action.pos % 256) as u8,
                value: action.value,
            },
            _ => unreachable!(),
        }
    }
}
