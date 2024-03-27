use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};

use fxhash::FxHashMap;
use loro::{
    event::Diff, Container, ContainerID, ContainerType, FracIndex, LoroDoc, LoroError, LoroTree,
    LoroValue, TreeExternalDiff, TreeID,
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
                // println!("\nbefore {:?}", tree.lock().unwrap().as_map().unwrap());
                // println!(
                //     "{:?}",
                //     event.events.iter().map(|e| &e.diff).collect::<Vec<_>>()
                // );
                tree.lock().unwrap().apply_diff(event);
                // println!("after {:?}\n", tree.lock().unwrap().as_map().unwrap());
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

#[derive(Debug)]
pub struct TreeTracker(Vec<TreeNode>);

impl ApplyDiff for TreeTracker {
    fn empty() -> Self {
        TreeTracker(Vec::new())
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_tree().unwrap();
        for diff in &diff.diff {
            let target = diff.target;
            match &diff.action {
                TreeExternalDiff::Create { parent, position } => {
                    let node = TreeNode::new(target, *parent, position.clone());
                    self.push(node);
                }
                TreeExternalDiff::Delete => {
                    self.retain(|node| node.id != target && node.parent != Some(target));
                }
                TreeExternalDiff::Move { parent, position } => {
                    let node = self.iter_mut().find(|node| node.id == target).unwrap();
                    node.parent = *parent;
                    node.position = position.clone();
                }
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        let mut list: Vec<FxHashMap<_, _>> = Vec::new();
        for node in self.iter() {
            let mut map = FxHashMap::default();
            map.insert("id".to_string(), node.id.to_string().into());
            map.insert("meta".to_string(), node.meta.to_value());
            map.insert(
                "parent".to_string(),
                match node.parent {
                    Some(parent) => parent.to_string().into(),
                    None => LoroValue::Null,
                },
            );
            map.insert("position".to_string(), node.position.to_string().into());
            list.push(map);
        }
        // compare by peer and then counter
        list.sort_by_key(|x| {
            let parent = if let LoroValue::String(p) = x.get("parent").unwrap() {
                Some(p.clone())
            } else {
                None
            };
            let index = x.get("position").unwrap().as_string().unwrap();
            (parent, index.clone())
        });
        list.into()
    }
}

impl Deref for TreeTracker {
    type Target = Vec<TreeNode>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for TreeTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug)]
pub struct TreeNode {
    pub id: TreeID,
    pub meta: ContainerTracker,
    pub parent: Option<TreeID>,
    pub position: FracIndex,
}

impl TreeNode {
    pub fn new(id: TreeID, parent: Option<TreeID>, position: FracIndex) -> Self {
        TreeNode {
            id,
            meta: ContainerTracker::Map(MapTracker::empty()),
            parent,
            position,
        }
    }
}
