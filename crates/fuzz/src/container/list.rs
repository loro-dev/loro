use std::sync::{Arc, Mutex};

use loro::{Container, ContainerID, ContainerType, LoroDoc, LoroList};
use tracing::debug_span;

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{ActionExecutor, ActorTrait},
    crdt_fuzzer::FuzzValue,
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

#[derive(Debug, Clone)]
pub enum ListAction {
    Insert { pos: u8, value: FuzzValue },
    Delete { pos: u8, len: u8 },
}

pub struct ListActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroList>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl ListActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty(ContainerID::new_root("sys:root", ContainerType::Map));
        tracker.insert(
            "list".to_string(),
            Value::empty_container(
                ContainerType::List,
                ContainerID::new_root("list", ContainerType::List),
            ),
        );
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(tracker)));
        let list = tracker.clone();

        let peer_id = loro.peer_id();
        loro.subscribe(
            &ContainerID::new_root("list", ContainerType::List),
            Arc::new(move |event| {
                let s = debug_span!("List event", peer = peer_id);
                let _g = s.enter();
                let mut list = list.lock().unwrap();
                list.apply_diff(event);
            }),
        )
        .detach();

        let root = loro.get_list("list");
        Self {
            loro,
            containers: vec![root],
            tracker,
        }
    }

    pub fn get_create_container_mut(&mut self, container_idx: usize) -> &mut LoroList {
        if self.containers.is_empty() {
            let handler = self.loro.get_list("list");
            self.containers.push(handler);
            self.containers.last_mut().unwrap()
        } else {
            self.containers.get_mut(container_idx).unwrap()
        }
    }
}

impl ActorTrait for ListActor {
    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }

    fn check_tracker(&self) {
        let list = self.loro.get_list("list");
        let value = list.get_deep_value();
        let tracker = self.tracker.lock().unwrap().to_value();
        assert_eq!(&value, tracker.into_map().unwrap().get("list").unwrap());
    }

    fn add_new_container(&mut self, container: Container) {
        self.containers.push(container.into_list().unwrap());
    }
}

impl Actionable for ListAction {
    fn pre_process(&mut self, actor: &mut ActionExecutor, container: usize) {
        let actor = actor.as_list_actor().unwrap();
        let list = actor.containers.get(container).unwrap();
        let length = list.len();

        if let ListAction::Insert { pos, .. } = self {
            *pos %= length.max(1) as u8;
        } else if length == 0 {
            *self = ListAction::Insert {
                pos: 0,
                value: FuzzValue::I32(0),
            };
        } else {
            let ListAction::Delete { pos, len } = self else {
                unreachable!()
            };
            *pos %= length.max(1) as u8;
            *len %= length as u8 - *pos;
        }
    }

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let actor = actor.as_list_actor_mut().unwrap();
        let list = actor.get_create_container_mut(container);
        match self {
            ListAction::Insert { pos, value } => {
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
            ListAction::Delete { pos, len } => {
                let pos = *pos as usize;
                let len = *len as usize;
                super::unwrap(list.delete(pos, len));
                None
            }
        }
    }

    fn ty(&self) -> ContainerType {
        ContainerType::List
    }

    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2] {
        match self {
            ListAction::Insert { pos, value } => {
                [format!("insert {pos}").into(), value.to_string().into()]
            }
            ListAction::Delete { pos, len } => {
                ["delete".into(), format!("{} ~ {}", pos, pos + len).into()]
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "List"
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        match self {
            ListAction::Insert { value, .. } => match value {
                FuzzValue::Container(c) => Some(c),
                _ => None,
            },
            ListAction::Delete { .. } => None,
        }
    }
}

impl FromGenericAction for ListAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        if action.bool {
            ListAction::Insert {
                pos: (action.pos % 256) as u8,
                value: action.value,
            }
        } else {
            ListAction::Delete {
                pos: (action.pos % 256) as u8,
                len: (action.length % 256) as u8,
            }
        }
    }
}
