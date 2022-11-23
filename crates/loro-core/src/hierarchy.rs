use std::fmt::Debug;

use fxhash::{FxHashMap, FxHashSet};

use crate::{
    container::{registry::ContainerRegistry, ContainerID},
    event::{Index, Observer, Path},
};

/// [`Hierarchy`] stores the hierarchical relationship between containers
#[derive(Default, Debug)]
pub(crate) struct Hierarchy {
    nodes: FxHashMap<ContainerID, Node>,
}

#[derive(Default)]
struct Node {
    parent: Option<ContainerID>,
    children: FxHashSet<ContainerID>,
    observers: Vec<Box<Observer>>,
}

impl Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("parent", &self.parent)
            .field("children", &self.children)
            .finish()
    }
}

impl Hierarchy {
    pub fn add_child(&mut self, parent: ContainerID, child: ContainerID) {
        let parent_node = self.nodes.entry(parent.clone()).or_default();
        parent_node.children.insert(child.clone());
        let child_node = self.nodes.entry(child).or_default();
        child_node.parent = Some(parent);
    }

    pub fn remove_child(&mut self, parent: &ContainerID, child: &ContainerID) {
        let parent_node = self.nodes.get_mut(parent).unwrap();
        parent_node.children.remove(child);
        let mut visited_descendants = FxHashSet::default();
        let mut stack = vec![child];
        while let Some(child) = stack.pop() {
            visited_descendants.insert(child.clone());
            let child_node = self.nodes.get(child).unwrap();
            for child in child_node.children.iter() {
                stack.push(child);
            }
        }

        for descendant in visited_descendants {
            self.nodes.remove(&descendant);
        }
    }

    pub fn get_path(
        &mut self,
        reg: &ContainerRegistry,
        descendant: &ContainerID,
        current_target: Option<&ContainerID>,
    ) -> Path {
        let mut path = Path::default();
        let node = Some(descendant);
        while let Some(node_id) = node {
            let node = self.nodes.get(node_id).unwrap();
            let parent = &node.parent;
            if let Some(parent) = parent {
                let parent_node = reg.get(parent).unwrap();
                let index = parent_node.lock().unwrap().index_of_child(node_id).unwrap();
                path.push(index);
            } else {
                match node_id {
                    ContainerID::Root {
                        name,
                        container_type,
                    } => path.push(Index::Key(name.clone())),
                    _ => unreachable!(),
                }
            }

            if parent.as_ref() == current_target {
                break;
            }
        }

        path
    }
}
