use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};

use fxhash::FxHashMap;
use loro::{
    event::Diff, Container, ContainerID, ContainerType, LoroDoc, LoroError, LoroTree, LoroValue,
    TreeExternalDiff, TreeID,
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
    Create { index: usize },
    Delete,
    Move { parent: (u64, i32), index: usize },
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
            *action = TreeActionInner::Create { index: 0 };
        }

        match action {
            TreeActionInner::Create { index } => {
                let id = tree.next_tree_id();
                let len = tree.children_len(None).unwrap_or(0);
                *index %= len + 1;
                *target = (id.peer, id.counter);
            }
            TreeActionInner::Delete => {
                let target_index = target.1 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
            }
            TreeActionInner::Move { parent, index } => {
                let target_index = target.1 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
                let mut parent_idx = parent.0 as usize % node_num;
                while target_index == parent_idx {
                    parent_idx = (parent_idx + 1) % node_num;
                }
                *parent = (nodes[parent_idx].peer, nodes[parent_idx].counter);
                *index %= tree
                    .children_len(Some(TreeID::new(parent.0, parent.1)))
                    .unwrap_or(0)
                    + 1;
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
            TreeActionInner::Create { index } => {
                tree.create_at(None, *index).unwrap();
                None
            }
            TreeActionInner::Delete => {
                tree.delete(target).unwrap();
                None
            }
            TreeActionInner::Move { parent, index } => {
                let parent = TreeID {
                    peer: parent.0,
                    counter: parent.1,
                };
                if let Err(LoroError::TreeError(_)) = tree.mov_to(target, Some(parent), *index) {
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
            TreeActionInner::Create { index } => [format!("create at {index}",).into(), target],
            TreeActionInner::Delete => ["delete".into(), target],
            TreeActionInner::Move {
                parent: (pi, pc),
                index,
            } => [format!("move to {pc}@{pi} at {index}").into(), target],
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
        let index = action.prop as usize;
        let action = match action.prop % 4 {
            0 => TreeActionInner::Create { index },
            1 => TreeActionInner::Delete,
            2 => TreeActionInner::Move { parent, index },
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

impl TreeTracker {
    pub(crate) fn find_node_by_id(&self, id: TreeID) -> Option<&TreeNode> {
        let mut s = VecDeque::from_iter(self.iter());
        while let Some(node) = s.pop_front() {
            if node.id == id {
                return Some(node);
            } else {
                s.extend(node.children.iter())
            }
        }
        None
    }

    pub(crate) fn find_node_by_id_mut(&mut self, id: TreeID) -> Option<&mut TreeNode> {
        let mut s = VecDeque::from_iter(self.iter_mut());
        while let Some(node) = s.pop_front() {
            if node.id == id {
                return Some(node);
            } else {
                s.extend(node.children.iter_mut())
            }
        }
        None
    }
}

impl ApplyDiff for TreeTracker {
    fn empty() -> Self {
        TreeTracker(Vec::new())
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_tree().unwrap();
        for diff in &diff.diff {
            let target = diff.target;
            match &diff.action {
                TreeExternalDiff::Create { parent, index } => {
                    let node = TreeNode::new(target, *parent);
                    if let Some(parent) = parent {
                        let parent = self.find_node_by_id_mut(*parent).unwrap();
                        parent.children.insert(*index, node);
                    } else {
                        self.insert(*index, node);
                    };
                }
                TreeExternalDiff::Delete => {
                    let node = self.find_node_by_id(target).unwrap();
                    if let Some(parent) = node.parent {
                        let parent = self.find_node_by_id_mut(parent).unwrap();
                        parent.children.retain(|n| n.id != target);
                    } else {
                        let index = self.iter().position(|n| n.id == target).unwrap();
                        self.0.remove(index);
                    };
                }
                TreeExternalDiff::Move { parent, index } => {
                    let node = self.find_node_by_id(target).unwrap();
                    let mut node = if let Some(p) = node.parent {
                        let parent = self.find_node_by_id_mut(p).unwrap();
                        let index = parent.children.iter().position(|n| n.id == target).unwrap();
                        parent.children.remove(index)
                    } else {
                        let index = self.iter().position(|n| n.id == target).unwrap();
                        self.0.remove(index)
                    };
                    node.parent = *parent;
                    if let Some(parent) = parent {
                        let parent = self.find_node_by_id_mut(*parent).unwrap();
                        parent.children.insert(*index, node);
                    } else {
                        self.insert(*index, node);
                    }
                }
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        let mut list: Vec<FxHashMap<_, _>> = Vec::new();
        for (i, node) in self.iter().enumerate() {
            node.to_value(i, &mut list);
        }

        list.sort_by_key(|x| {
            let parent = if let LoroValue::String(p) = x.get("parent").unwrap() {
                Some(p.clone())
            } else {
                None
            };

            (
                parent,
                *x.get("index").unwrap().as_i64().unwrap(),
                x.get("id").unwrap().as_string().unwrap().clone(),
            )
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
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(id: TreeID, parent: Option<TreeID>) -> Self {
        TreeNode {
            id,
            meta: ContainerTracker::Map(MapTracker::empty()),
            parent,
            children: vec![],
        }
    }

    fn to_value(&self, index: usize, list: &mut Vec<FxHashMap<String, LoroValue>>) {
        for (i, child) in self.children.iter().enumerate() {
            child.to_value(i, list);
        }
        let mut map = FxHashMap::default();
        map.insert("id".to_string(), self.id.to_string().into());
        map.insert("meta".to_string(), self.meta.to_value());
        map.insert(
            "parent".to_string(),
            match self.parent {
                Some(parent) => parent.to_string().into(),
                None => LoroValue::Null,
            },
        );
        map.insert("index".to_string(), (index as i64).into());
        list.push(map);
    }
}
