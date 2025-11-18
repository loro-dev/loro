use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use loro::{
    event::Diff, Container, ContainerID, ContainerType, LoroDoc, LoroError, LoroTree, LoroValue,
    TreeExternalDiff, TreeID,
};
use rustc_hash::FxHashMap;

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{assert_value_eq, ActionExecutor, ActorTrait},
    crdt_fuzzer::FuzzValue,
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

#[derive(Debug, Clone)]
pub struct TreeAction {
    pub target: (u64, i32),
    pub action: TreeActionInner,
}

#[derive(Clone)]
pub enum TreeActionInner {
    Create {
        index: usize,
    },
    Delete,
    Move {
        parent: (u64, i32),
        index: usize,
    },
    MoveBefore {
        target: (u64, i32),
        before: (u64, i32),
    },
    MoveAfter {
        target: (u64, i32),
        after: (u64, i32),
    },
    Meta {
        meta: (String, FuzzValue),
    },
}

impl Debug for TreeActionInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeActionInner::Create { index } => {
                write!(f, "TreeActionInner::Create{{index:{}}}", index)
            }
            TreeActionInner::Delete => write!(f, "TreeActionInner::Delete"),
            TreeActionInner::Move { parent, index } => {
                write!(
                    f,
                    "TreeActionInner::Move{{parent:{:?}, index:{}}}",
                    parent, index
                )
            }
            TreeActionInner::MoveBefore { target, before } => {
                write!(
                    f,
                    "TreeActionInner::MoveBefore{{target:{:?}, before:{:?}}}",
                    target, before
                )
            }
            TreeActionInner::MoveAfter { target, after } => {
                write!(
                    f,
                    "TreeActionInner::MoveAfter{{target:{:?}, after:{:?}}}",
                    target, after
                )
            }
            TreeActionInner::Meta { meta } => write!(
                f,
                "TreeActionInner::Meta{{meta:(\"{}\".into(),{:?})}}",
                meta.0, meta.1
            ),
        }
    }
}

pub struct TreeActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroTree>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl TreeActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty(ContainerID::new_root("sys:root", ContainerType::Map));
        tracker.insert(
            "tree".to_string(),
            Value::empty_container(
                ContainerType::Tree,
                ContainerID::new_root("tree", ContainerType::Tree),
            ),
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
        )
        .detach();

        let root = loro.get_tree("tree");
        root.enable_fractional_index(0);
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
        assert_value_eq(
            &result,
            tracker.into_map().unwrap().get("tree").unwrap(),
            None,
        );
    }

    fn add_new_container(&mut self, container: Container) {
        self.containers.push(container.into_tree().unwrap());
    }
}

impl Actionable for TreeAction {
    fn pre_process(&mut self, actor: &mut ActionExecutor, container: usize) {
        let actor = actor.as_tree_actor().unwrap();
        let tree = actor.containers.get(container).unwrap();
        let nodes = tree
            .nodes()
            .into_iter()
            .filter(|x| !tree.is_node_deleted(x).unwrap())
            .collect::<Vec<_>>();
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
                let id = tree.__internal__next_tree_id();
                let len = tree.children_num(None).unwrap_or(0);
                *index %= len + 1;
                *target = (id.peer, id.counter);
            }
            TreeActionInner::Delete => {
                let target_index = target.0 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
            }
            TreeActionInner::Move { parent, index } => {
                let target_index = target.0 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
                let mut parent_idx = parent.0 as usize % node_num;
                while target_index == parent_idx {
                    parent_idx = (parent_idx + 1) % node_num;
                }
                *parent = (nodes[parent_idx].peer, nodes[parent_idx].counter);
                *index %= tree
                    .children_num(TreeID::new(parent.0, parent.1))
                    .unwrap_or(0)
                    + 1;
            }
            TreeActionInner::MoveBefore {
                target,
                before: (p, c),
            }
            | TreeActionInner::MoveAfter {
                target,
                after: (p, c),
            } => {
                let target_index = target.0 as usize % node_num;
                *target = (nodes[target_index].peer, nodes[target_index].counter);
                let mut other_idx = *p as usize % node_num;
                while target_index == other_idx {
                    other_idx = (other_idx + 1) % node_num;
                }
                *p = nodes[other_idx].peer;
                *c = nodes[other_idx].counter;
            }
            TreeActionInner::Meta { meta: (_, v) } => {
                let target_index = target.0 as usize % node_num;
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
        tree.enable_fractional_index(0);
        match action {
            TreeActionInner::Create { index } => {
                super::unwrap(tree.create_at(None, *index));
                None
            }
            TreeActionInner::Delete => {
                super::unwrap(tree.delete(target));
                None
            }
            TreeActionInner::Move { parent, index } => {
                let parent = TreeID {
                    peer: parent.0,
                    counter: parent.1,
                };
                if let Err(LoroError::TreeError(e)) = tree.mov_to(target, Some(parent), *index) {
                    // cycle move
                    tracing::warn!("move error {}", e);
                }
                None
            }
            TreeActionInner::MoveBefore { target, before } => {
                let target = TreeID {
                    peer: target.0,
                    counter: target.1,
                };
                let before = TreeID {
                    peer: before.0,
                    counter: before.1,
                };
                super::unwrap(tree.mov_before(target, before));
                None
            }
            TreeActionInner::MoveAfter { target, after } => {
                let target = TreeID {
                    peer: target.0,
                    counter: target.1,
                };
                let after = TreeID {
                    peer: after.0,
                    counter: after.1,
                };
                super::unwrap(tree.mov_after(target, after));
                None
            }
            TreeActionInner::Meta { meta: (k, v) } => {
                let meta = super::unwrap(tree.get_meta(target))?;
                match v {
                    FuzzValue::I32(i) => {
                        super::unwrap(meta.insert(k, LoroValue::from(*i)));
                        None
                    }
                    FuzzValue::Container(c) => {
                        super::unwrap(meta.insert_container(k, Container::new(*c)))
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
            TreeActionInner::MoveBefore {
                target: (ti, tc),
                before: (bi, bc),
            } => [format!("move {tc}@{ti} before {bc}@{bi}").into(), target],
            TreeActionInner::MoveAfter {
                target: (ti, tc),
                after: (ai, ac),
            } => [format!("move {tc}@{ti} after {ac}@{ai}").into(), target],
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
            4 => TreeActionInner::MoveBefore {
                target,
                before: parent,
            },
            5 => TreeActionInner::MoveAfter {
                target,
                after: parent,
            },
            _ => unreachable!(),
        };
        Self { target, action }
    }
}

#[derive(Debug)]
pub struct TreeTracker {
    id: ContainerID,
    tree: Vec<TreeNode>,
}

impl TreeTracker {
    pub(crate) fn find_node_by_id(&self, id: TreeID) -> Option<&TreeNode> {
        let mut s = VecDeque::from_iter(self.tree.iter());
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
        let mut s = VecDeque::from_iter(self.tree.iter_mut());
        while let Some(node) = s.pop_front() {
            if node.id == id {
                return Some(node);
            } else {
                s.extend(node.children.iter_mut())
            }
        }
        None
    }

    fn create_node(
        &mut self,
        target: TreeID,
        parent: &Option<TreeID>,
        position: String,
        index: &usize,
    ) {
        let node = TreeNode::new(target, *parent, position);
        if let Some(parent) = parent {
            let parent = self.find_node_by_id_mut(*parent).unwrap();
            parent.children.insert(*index, node);
        } else {
            if self.find_node_by_id_mut(target).is_some() {
                panic!("{:?} node already exists", target);
            }

            self.tree.insert(*index, node);
        };
    }
}

impl ApplyDiff for TreeTracker {
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn empty(id: ContainerID) -> Self {
        TreeTracker { id, tree: vec![] }
    }

    fn apply_diff(&mut self, diff: Diff) {
        // trace!("current tree: {:#?}", &self.tree);
        // trace!("applying diff: {:#?}", &diff);
        let diff = diff.as_tree().unwrap();
        for diff in &diff.diff {
            let target = diff.target;
            match &diff.action {
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position,
                } => {
                    if self.find_node_by_id(target).is_some() {
                        panic!("{:?} node already exists", target);
                    }

                    self.create_node(target, &parent.tree_id(), position.to_string(), index);
                }
                TreeExternalDiff::Delete { .. } => {
                    let node = self.find_node_by_id(target).unwrap();
                    if let Some(parent) = node.parent {
                        let parent = self.find_node_by_id_mut(parent).unwrap();
                        parent.children.retain(|n| n.id != target);
                    } else {
                        let index = self.tree.iter().position(|n| n.id == target).unwrap();
                        self.tree.remove(index);
                    };
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position,
                    ..
                } => {
                    let Some(node) = self.find_node_by_id(target) else {
                        // self.create_node(target, &parent.tree_id(), position.to_string(), index);
                        // continue;
                        panic!("Expected move but the node needs to be created");
                    };

                    let mut node = if let Some(p) = node.parent {
                        let parent = self.find_node_by_id_mut(p).unwrap();
                        let index = parent.children.iter().position(|n| n.id == target).unwrap();
                        parent.children.remove(index)
                    } else {
                        let index = self.tree.iter().position(|n| n.id == target).unwrap();
                        self.tree.remove(index)
                    };
                    node.parent = parent.tree_id();
                    node.position = position.to_string();
                    if let Some(parent) = parent.tree_id() {
                        let parent = self.find_node_by_id_mut(parent).unwrap();
                        parent.children.insert(*index, node);
                    } else {
                        if self.find_node_by_id_mut(target).is_some() {
                            panic!("{:?} node already exists", target);
                        }

                        self.tree.insert(*index, node);
                    }
                }
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        let mut list: Vec<FxHashMap<_, _>> = Vec::new();
        for (i, node) in self.tree.iter().enumerate() {
            list.push(node.to_value(i));
        }
        list.into()
    }
}

#[derive(Debug)]
pub struct TreeNode {
    pub id: TreeID,
    pub meta: ContainerTracker,
    pub parent: Option<TreeID>,
    pub position: String,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(id: TreeID, parent: Option<TreeID>, position: String) -> Self {
        TreeNode {
            id,
            meta: ContainerTracker::Map(MapTracker::empty(id.associated_meta_container())),
            parent,
            position,
            children: vec![],
        }
    }

    fn to_value(&self, index: usize) -> FxHashMap<String, LoroValue> {
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
        map.insert("fractional_index".to_string(), self.position.clone().into());
        map.insert("index".to_string(), (index as i64).into());
        map.insert(
            "children".to_string(),
            self.children
                .iter()
                .enumerate()
                .map(|(i, n)| n.to_value(i))
                .collect::<Vec<_>>()
                .into(),
        );
        map
    }
}
