use std::fmt::Debug;

use fxhash::{FxHashMap, FxHashSet};

use crate::{
    configure::rand_u64,
    container::{registry::ContainerRegistry, ContainerID},
    event::{Event, Index, Observer, Path, RawEvent, SubscriptionID},
};

/// [`Hierarchy`] stores the hierarchical relationship between containers
#[derive(Default, Debug)]
pub struct Hierarchy {
    nodes: FxHashMap<ContainerID, Node>,
}

#[derive(Default)]
struct Node {
    parent: Option<ContainerID>,
    children: FxHashSet<ContainerID>,
    observers: FxHashMap<SubscriptionID, Observer>,
    deep_observers: FxHashMap<SubscriptionID, Observer>,
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
    #[inline(always)]
    pub fn new() -> Self {
        Default::default()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn add_child(&mut self, parent: &ContainerID, child: &ContainerID) {
        let parent_node = self.nodes.entry(parent.clone()).or_default();
        parent_node.children.insert(child.clone());
        let child_node = self.nodes.entry(child.clone()).or_default();
        child_node.parent = Some(parent.clone());
    }

    #[inline(always)]
    pub fn has_children(&self, id: &ContainerID) -> bool {
        self.nodes
            .get(id)
            .map(|node| !node.children.is_empty())
            .unwrap_or(false)
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
                        container_type: _,
                    } => path.push(Index::Key(name.clone())),
                    _ => unreachable!(),
                }
            }

            if parent.as_ref() == current_target {
                break;
            }
        }

        path.reverse();
        path
    }

    pub fn should_notify(&self, container_id: &ContainerID) -> bool {
        let mut node_id = Some(container_id);
        while let Some(inner_node_id) = node_id {
            let Some(node) = self.nodes.get(inner_node_id) else {
                return false;
            };

            if !node.observers.is_empty() {
                return true;
            }

            node_id = node.parent.as_ref();
        }

        false
    }

    pub fn notify(&mut self, raw_event: RawEvent, reg: &ContainerRegistry) {
        let target_id = raw_event.container_id;
        let mut absolute_path = self.get_path(reg, &target_id, None);
        absolute_path.reverse();
        let path_to_root = absolute_path;
        let mut current_target_id = Some(&target_id);
        let mut count = 0;
        let mut event = Event {
            relative_path: Default::default(),
            old_version: raw_event.old_version,
            new_version: raw_event.new_version,
            current_target: target_id.clone(),
            target: target_id.clone(),
            diff: raw_event.diff,
            local: raw_event.local,
        };

        let node = self.nodes.get(&target_id).unwrap();
        if !node.observers.is_empty() {
            for (_, observer) in node.observers.iter() {
                observer(&event);
            }
        }

        while let Some(id) = current_target_id {
            let node = self.nodes.get(id).unwrap();
            if !node.deep_observers.is_empty() {
                let mut relative_path = path_to_root[..count].to_vec();
                relative_path.reverse();
                event.relative_path = relative_path;
                event.current_target = id.clone();
                for (_, observer) in node.deep_observers.iter() {
                    observer(&event);
                }
            }

            count += 1;
            current_target_id = node.parent.as_ref();
        }
    }

    pub fn subscribe(&mut self, container: &ContainerID, observer: Observer) -> SubscriptionID {
        let id = rand_u64();
        self.nodes
            .entry(container.clone())
            .or_default()
            .observers
            .insert(id, observer);
        id
    }

    pub fn unsubscribe(&mut self, container: &ContainerID, id: SubscriptionID) {
        if let Some(x) = self.nodes.get_mut(container) {
            x.observers.remove(&id);
        } else {
            // TODO: warning
        }
    }
}
