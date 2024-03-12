use std::sync::{Arc, Mutex};

use loro::{
    Container, ContainerID, ContainerType, LoroDoc, LoroError, LoroTree, LoroValue, TreeID,
};

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{ActionExecutor, ActorTrait},
    crdt_fuzzer::FuzzValue,
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

#[derive(Debug, Clone)]
pub struct TreeAction {
    target: (u64, i32),
    action: TreeActionInner,
}

#[derive(Debug, Clone)]
pub enum TreeActionInner {
    Create,
    Delete,
    Move { parent: (u64, i32) },
    Meta { meta: (String, FuzzValue) },
}

pub struct TreeActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroTree>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl TreeActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty();
        tracker.insert(
            "tree".to_string(),
            Value::empty_container(ContainerType::Tree),
        );
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(tracker)));
        let tree = tracker.clone();
        loro.subscribe(
            &ContainerID::new_root("tree", ContainerType::Tree),
            Arc::new(move |event| {
                tree.lock().unwrap().apply_diff(event);
            }),
        );

        let root = loro.get_tree("tree");
        Self {
            loro,
            containers: vec![root],
            tracker,
        }
    }
}

impl ActorTrait for TreeActor {
    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }

    fn check_tracker(&self) {
        let loro = &self.loro;
        let tree = loro.get_tree("tree");
        let result = tree.get_value_with_meta();
        let tracker = self.tracker.lock().unwrap().to_value();
        assert_eq!(&result, tracker.into_map().unwrap().get("tree").unwrap());
    }

    fn add_new_container(&mut self, container: Container) {
        self.containers.push(container.into_tree().unwrap());
    }
}

impl Actionable for TreeAction {
    fn pre_process(&mut self, actor: &mut ActionExecutor, container: usize) {
        let actor = actor.as_tree_actor().unwrap();
        let tree = actor.containers.get(container).unwrap();
        let nodes = tree.nodes();
        let node_num = nodes.len();
        let TreeAction { target, action } = self;
        if node_num == 0
            || node_num < 2
                && (matches!(
                    action,
                    TreeActionInner::Move { .. } | TreeActionInner::Meta { .. }
                ))
        {
            *action = TreeActionInner::Create;
        }

        match action {
            TreeActionInner::Create => {
                let id = tree.next_tree_id();
                *target = (id.peer, id.counter);
            }
            TreeActionInner::Delete => {
                let target_index = target.1 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
            }
            TreeActionInner::Move { parent } => {
                let target_index = target.1 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
                let mut parent_idx = parent.0 as usize % node_num;
                while target_index == parent_idx {
                    parent_idx = (parent_idx + 1) % node_num;
                }
                *parent = (nodes[parent_idx].peer, nodes[parent_idx].counter);
            }
            TreeActionInner::Meta { meta: (_, v) } => {
                let target_index = target.1 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
                if matches!(v, FuzzValue::Container(_)) {
                    *v = FuzzValue::I32(0);
                }
            }
        }
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        if let TreeActionInner::Meta {
            meta: (_, FuzzValue::Container(c)),
        } = &mut self.action
        {
            Some(c)
        } else {
            None
        }
    }

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let tree = actor
            .as_tree_actor_mut()
            .unwrap()
            .containers
            .get_mut(container)
            .unwrap();
        let TreeAction { target, action } = self;
        let target = TreeID {
            peer: target.0,
            counter: target.1,
        };
        match action {
            TreeActionInner::Create => {
                tree.create(None).unwrap();
                None
            }
            TreeActionInner::Delete => {
                tree.delete(target).unwrap();
                None
            }
            TreeActionInner::Move { parent } => {
                let parent = TreeID {
                    peer: parent.0,
                    counter: parent.1,
                };
                if let Err(LoroError::TreeError(_)) = tree.mov(target, Some(parent)) {
                    // cycle move
                }
                None
            }
            TreeActionInner::Meta { meta: (k, v) } => {
                let meta = tree.get_meta(target).unwrap();
                match v {
                    FuzzValue::I32(i) => {
                        meta.insert(k, LoroValue::from(*i)).unwrap();
                        None
                    }
                    FuzzValue::Container(c) => {
                        let container = meta.insert_container(k, *c).unwrap();
                        Some(container)
                    }
                }
            }
        }
    }

    fn ty(&self) -> ContainerType {
        ContainerType::Tree
    }

    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2] {
        let target = format!("{}@{}", self.target.1, self.target.0).into();
        match &self.action {
            TreeActionInner::Create => ["create".into(), target],
            TreeActionInner::Delete => ["delete".into(), target],
            TreeActionInner::Move { parent: (pi, pc) } => {
                [format!("move to {pc}@{pi}").into(), target]
            }
            TreeActionInner::Meta { meta } => [format!("meta\n {:?}", meta).into(), target],
        }
    }

    fn type_name(&self) -> &'static str {
        "Tree"
    }
}

impl FromGenericAction for TreeAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        let target = (action.pos as u64, 0);
        let parent = (action.length as u64, 0);
        let action = match action.prop % 4 {
            0 => TreeActionInner::Create,
            1 => TreeActionInner::Delete,
            2 => TreeActionInner::Move { parent },
            3 => TreeActionInner::Meta {
                meta: (action.key.to_string(), action.value),
            },
            _ => unreachable!(),
        };
        Self { target, action }
    }
}
