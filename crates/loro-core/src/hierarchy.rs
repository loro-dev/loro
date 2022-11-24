use std::fmt::Debug;

use fxhash::{FxHashMap, FxHashSet};

use crate::{
    configure::rand_u64,
    container::{registry::ContainerRegistry, ContainerID},
    event::{Event, Index, Observer, Path, RawEvent, SubscriptionID},
};

/// [`Hierarchy`] stores the hierarchical relationship between containers
#[derive(Default)]
pub struct Hierarchy {
    nodes: FxHashMap<ContainerID, Node>,
    root_observers: FxHashMap<SubscriptionID, Observer>,
    deleted: FxHashSet<ContainerID>,
}

#[derive(Default)]
struct Node {
    parent: Option<ContainerID>,
    children: FxHashSet<ContainerID>,
    observers: FxHashMap<SubscriptionID, Observer>,
    deep_observers: FxHashMap<SubscriptionID, Observer>,
}

impl Debug for Hierarchy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hierarchy")
            .field("nodes", &self.nodes)
            .finish()
    }
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
        let Some(parent_node) = self.nodes.get_mut(parent) else {
            return;
        };
        parent_node.children.remove(child);
        let mut visited_descendants = FxHashSet::default();
        let mut stack = vec![child];
        while let Some(child) = stack.pop() {
            visited_descendants.insert(child.clone());
            let Some(child_node) = self.nodes.get(child) else {
                continue;
            };
            for child in child_node.children.iter() {
                stack.push(child);
            }
        }

        for descendant in visited_descendants.iter() {
            self.nodes.remove(descendant);
        }

        self.deleted.extend(visited_descendants);
    }

    pub fn take_deleted(&mut self) -> FxHashSet<ContainerID> {
        std::mem::take(&mut self.deleted)
    }

    pub fn get_path(
        &mut self,
        reg: &ContainerRegistry,
        descendant: &ContainerID,
        target: Option<&ContainerID>,
    ) -> Path {
        if let ContainerID::Root { name, .. } = descendant {
            return vec![Index::Key(name.into())];
        }

        if target.map(|x| x == descendant).unwrap_or(false) {
            return vec![];
        }

        let mut path = Path::default();
        let mut iter_node = Some(descendant);
        while let Some(node_id) = iter_node {
            let Some(node) = self.nodes.get(node_id) else {
                debug_assert!(node_id.is_root());
                break;
            };
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

            if parent.as_ref() == target {
                break;
            }

            iter_node = parent.as_ref();
        }

        path.reverse();
        path
    }

    pub fn should_notify(&self, container_id: &ContainerID) -> bool {
        if !self.root_observers.is_empty() {
            return true;
        }

        if self
            .nodes
            .get(container_id)
            .map(|x| !x.observers.is_empty())
            .unwrap_or(false)
        {
            return true;
        }

        let mut node_id = Some(container_id);
        while let Some(inner_node_id) = node_id {
            let Some(node) = self.nodes.get(inner_node_id) else {
                return false;
            };

            if !node.deep_observers.is_empty() {
                return true;
            }

            node_id = node.parent.as_ref();
        }

        false
    }

    pub fn notify_batch(&mut self, raw_events: Vec<RawEvent>, reg: &ContainerRegistry) {
        // notify event in the order of path length
        // otherwise, the paths to children may be incorrect when the parents are affected by some of the events
        let mut event_and_paths = raw_events
            .into_iter()
            .map(|x| (self.get_path(reg, &x.container_id, None), x))
            .collect::<Vec<_>>();
        event_and_paths.sort_by_cached_key(|x| x.0.len());
        for (path, event) in event_and_paths {
            self.notify_with_path(event, path);
        }
    }

    pub fn notify(&mut self, raw_event: RawEvent, reg: &ContainerRegistry) {
        let absolute_path = self.get_path(reg, &raw_event.container_id, None);
        self.notify_with_path(raw_event, absolute_path);
    }

    fn notify_with_path(&mut self, raw_event: RawEvent, absolute_path: Path) {
        let target_id = raw_event.container_id;
        let mut event = Event {
            absolute_path,
            relative_path: Default::default(),
            old_version: raw_event.old_version,
            new_version: raw_event.new_version,
            current_target: Some(target_id.clone()),
            target: target_id.clone(),
            diff: raw_event.diff,
            local: raw_event.local,
        };
        let mut current_target_id = Some(target_id.clone());
        let mut count = 0;
        let mut path_to_root = event.absolute_path.clone();
        path_to_root.reverse();
        let node = self.nodes.entry(target_id).or_default();
        if !node.observers.is_empty() {
            for (_, observer) in node.observers.iter_mut() {
                observer(&event);
            }
        }
        while let Some(id) = current_target_id {
            let node = self.nodes.get_mut(&id).unwrap();
            if !node.deep_observers.is_empty() {
                let mut relative_path = path_to_root[..count].to_vec();
                relative_path.reverse();
                event.relative_path = relative_path;
                event.current_target = Some(id.clone());
                for (_, observer) in node.deep_observers.iter_mut() {
                    observer(&event);
                }
            }

            count += 1;
            if node.parent.is_none() {
                debug_assert!(id.is_root());
            }

            current_target_id = node.parent.as_ref().cloned();
        }
        if !self.root_observers.is_empty() {
            event.relative_path = event.absolute_path.clone();
            event.current_target = None;
            for (_, observer) in self.root_observers.iter_mut() {
                observer(&event);
            }
        }
    }

    pub fn subscribe(
        &mut self,
        container: &ContainerID,
        observer: Observer,
        deep: bool,
    ) -> SubscriptionID {
        let id = rand_u64();
        if deep {
            self.nodes
                .entry(container.clone())
                .or_default()
                .deep_observers
                .insert(id, observer);
        } else {
            self.nodes
                .entry(container.clone())
                .or_default()
                .observers
                .insert(id, observer);
        }
        id
    }

    pub fn unsubscribe(&mut self, container: &ContainerID, id: SubscriptionID) -> bool {
        if let Some(x) = self.nodes.get_mut(container) {
            x.observers.remove(&id).is_some()
        } else {
            // TODO: warning
            false
        }
    }

    pub fn subscribe_root(&mut self, observer: Observer) -> SubscriptionID {
        let id = rand_u64();
        self.root_observers.insert(id, observer);
        id
    }

    pub fn unsubscribe_root(&mut self, sub: SubscriptionID) -> bool {
        self.root_observers.remove(&sub).is_some()
    }
}
