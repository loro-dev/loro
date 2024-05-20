use std::collections::VecDeque;

use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, ContainerType, Counter, LoroResult, LoroTreeError, LoroValue, PeerID, TreeID,
};

use crate::{
    container::tree::tree_op::TreeOp,
    delta::{TreeDiffItem, TreeExternalDiff},
    state::{FractionalIndexGenResult, TreeParentId},
    txn::{EventHint, Transaction},
    BasicHandler, HandlerTrait, MapHandler,
};

use super::{create_handler, Handler, MaybeDetached};

#[derive(Clone)]
pub struct TreeHandler {
    pub(super) inner: MaybeDetached<TreeInner>,
}

#[derive(Clone)]
pub(super) struct TreeInner {
    next_counter: Counter,
    map: FxHashMap<TreeID, MapHandler>,
    parent_links: FxHashMap<TreeID, Option<TreeID>>,
    children_links: FxHashMap<Option<TreeID>, Vec<TreeID>>,
}

impl TreeInner {
    fn new() -> Self {
        TreeInner {
            next_counter: 0,
            map: FxHashMap::default(),
            parent_links: FxHashMap::default(),
            children_links: FxHashMap::default(),
        }
    }

    fn create(&mut self, parent: Option<TreeID>, index: usize) -> TreeID {
        let id = TreeID::new(PeerID::MAX, self.next_counter);
        self.next_counter += 1;
        self.map.insert(id, MapHandler::new_detached());
        self.parent_links.insert(id, parent);
        let children = self.children_links.entry(parent).or_default();
        children.insert(index, id);
        id
    }

    fn create_with_target(
        &mut self,
        parent: Option<TreeID>,
        index: usize,
        target: TreeID,
    ) -> TreeID {
        self.map.insert(target, MapHandler::new_detached());
        self.parent_links.insert(target, parent);
        let children = self.children_links.entry(parent).or_default();
        children.insert(index, target);
        target
    }

    fn mov(&mut self, target: TreeID, new_parent: Option<TreeID>, index: usize) -> LoroResult<()> {
        let old_parent = self
            .parent_links
            .get_mut(&target)
            .ok_or(LoroTreeError::TreeNodeNotExist(target))?;
        let children = self.children_links.get_mut(old_parent).unwrap();
        children.retain(|x| x != &target);
        self.parent_links.insert(target, new_parent);
        let children = self.children_links.entry(new_parent).or_default();
        children.insert(index, target);
        Ok(())
    }

    fn delete(&mut self, id: TreeID) -> LoroResult<()> {
        self.map.remove(&id);
        let parent = self
            .parent_links
            .remove(&id)
            .ok_or(LoroTreeError::TreeNodeNotExist(id))?;
        let children = self.children_links.get_mut(&parent).unwrap();
        children.retain(|x| x != &id);
        self.children_links.remove(&Some(id));
        Ok(())
    }

    fn get_id_by_index(&self, parent: &Option<TreeID>, index: usize) -> Option<TreeID> {
        self.children_links
            .get(parent)
            .and_then(|x| x.get(index).cloned())
    }

    fn get_parent(&self, id: &TreeID) -> Option<Option<TreeID>> {
        self.parent_links.get(id).cloned()
    }

    fn get_children(&self, parent: Option<TreeID>) -> Option<Vec<TreeID>> {
        self.children_links.get(&parent).cloned()
    }

    fn children_num(&self, parent: Option<TreeID>) -> Option<usize> {
        self.children_links.get(&parent).map(|x| x.len())
    }

    fn is_parent(&self, target: TreeID, parent: Option<TreeID>) -> bool {
        self.parent_links.get(&target) == Some(&parent)
    }

    fn get_index_by_tree_id(&self, target: &TreeID) -> Option<usize> {
        self.parent_links
            .get(target)
            .and_then(|parent| self.children_links.get(parent))
            .and_then(|children| children.iter().position(|x| x == target))
    }

    fn get_value(&self, deep: bool) -> LoroValue {
        let mut ans = vec![];

        let mut q = VecDeque::from_iter(
            self.children_links
                .get(&None)
                .unwrap()
                .iter()
                .enumerate()
                .zip(std::iter::repeat(None::<TreeID>)),
        );
        while let Some(((idx, target), parent)) = q.pop_front() {
            let map = self.map.get(target).unwrap();
            let mut loro_map_value = FxHashMap::default();
            loro_map_value.insert("id".to_string(), target.to_string().into());
            let parent = parent
                .map(|p| p.to_string().into())
                .unwrap_or(LoroValue::Null);
            loro_map_value.insert("parent".to_string(), parent);
            loro_map_value.insert(
                "meta".to_string(),
                if deep {
                    map.get_deep_value()
                } else {
                    String::from("UnResolved").into()
                },
            );
            loro_map_value.insert("index".to_string(), (idx as i64).into());
            ans.push(loro_map_value);
            if let Some(children) = self.children_links.get(&Some(*target)) {
                for (idx, child) in children.iter().enumerate() {
                    q.push_back(((idx, child), Some(*target)));
                }
            }
        }
        ans.into()
    }
}

impl HandlerTrait for TreeHandler {
    fn to_handler(&self) -> Handler {
        Handler::Tree(self.clone())
    }

    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                let inner = create_handler(parent, self_id);
                let tree = inner.into_tree().unwrap();

                let children = t.value.children_links.get(&None);
                let mut q = children
                    .map(|c| {
                        VecDeque::from_iter(
                            c.iter().enumerate().zip(std::iter::repeat(None::<TreeID>)),
                        )
                    })
                    .unwrap_or_default();
                while let Some(((idx, target), parent)) = q.pop_front() {
                    let real_id = tree.create_with_txn(txn, parent, idx)?;
                    let map = t.value.map.get(target).unwrap();
                    map.attach(
                        txn,
                        tree.inner.try_attached_state()?,
                        real_id.associated_meta_container(),
                    )?;

                    if let Some(children) = t.value.children_links.get(&Some(*target)) {
                        for (idx, child) in children.iter().enumerate() {
                            q.push_back(((idx, child), Some(real_id)));
                        }
                    }
                }
                Ok(tree)
            }
            MaybeDetached::Attached(a) => {
                let new_inner = create_handler(a, self_id);
                let ans = new_inner.into_tree().unwrap();
                let tree_nodes = ans.with_state(|s| Ok(s.as_tree_state().unwrap().tree_nodes()))?;
                for node in tree_nodes {
                    let parent = node.parent;
                    let index = node.index;
                    let target = node.id;
                    let real_id = ans.create_with_txn(txn, parent, index)?;
                    ans.get_meta(target)?
                        .attach(txn, a, real_id.associated_meta_container())?;
                }
                Ok(ans)
            }
        }
    }

    fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        self.inner.attached_handler()
    }

    // TODO:
    fn get_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_value(false)
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_value(true)
            }
            MaybeDetached::Attached(a) => a.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        ContainerType::Tree
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.lock().unwrap().attached.clone().map(|x| Self {
                inner: MaybeDetached::Attached(x),
            }),
            MaybeDetached::Attached(_a) => Some(self.clone()),
        }
    }

    fn from_handler(h: Handler) -> Option<Self> {
        match h {
            Handler::Tree(x) => Some(x),
            _ => None,
        }
    }
}

impl std::fmt::Debug for TreeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => write!(f, "TreeHandler Dettached"),
            MaybeDetached::Attached(a) => write!(f, "TreeHandler {}", a.id),
        }
    }
}

impl TreeHandler {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted/synced.
    /// To attach the container to the document, please insert it into an attached
    /// container.
    pub fn new_detached() -> Self {
        Self {
            inner: MaybeDetached::new_detached(TreeInner::new()),
        }
    }

    pub fn delete(&self, target: TreeID) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                t.value.delete(target)?;
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, target)),
        }
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, target: TreeID) -> LoroResult<()> {
        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target,
                parent: Some(TreeID::delete_root()),
                position: None,
            }),
            EventHint::Tree(TreeDiffItem {
                target,
                action: TreeExternalDiff::Delete {
                    old_parent: self
                        .get_node_parent(&target)
                        .map(TreeParentId::from)
                        .unwrap_or(TreeParentId::Unexist),
                    old_index: self.get_index_by_tree_id(&target).unwrap_or(0),
                },
            }),
            &inner.state,
        )
    }

    pub fn create<T: Into<Option<TreeID>>>(&self, parent: T) -> LoroResult<TreeID> {
        let parent = parent.into();
        let index: usize = self.children_num(parent).unwrap_or(0);
        self.create_at(parent, index)
    }

    pub fn create_at<T: Into<Option<TreeID>>>(
        &self,
        parent: T,
        index: usize,
    ) -> LoroResult<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = &mut t.try_lock().unwrap().value;
                Ok(t.create(parent.into(), index))
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.create_with_txn(txn, parent, index))
            }
        }
    }

    /// For undo/redo, Specify the TreeID of the created node
    pub(crate) fn create_at_with_target(
        &self,
        parent: Option<TreeID>,
        index: usize,
        target: TreeID,
    ) -> LoroResult<()> {
        if let Some(p) = parent {
            if !self.contains(p) {
                return Ok(());
            }
        }
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = &mut t.try_lock().unwrap().value;
                t.create_with_target(parent, index, target);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                let inner = self.inner.try_attached_state()?;
                match self.generate_position_at(&target, parent, index) {
                    FractionalIndexGenResult::Ok(position) => {
                        self.create_with_position(inner, txn, target, parent, index, position)?;
                    }
                    FractionalIndexGenResult::Rearrange(ids) => {
                        for (i, (id, position)) in ids.into_iter().enumerate() {
                            if i == 0 {
                                self.create_with_position(inner, txn, id, parent, index, position)?;
                                continue;
                            }
                            self.mov_with_position(inner, txn, id, parent, index + i, position)?;
                        }
                    }
                };
                Ok(())
            }),
        }
    }

    pub fn create_with_txn<T: Into<Option<TreeID>>>(
        &self,
        txn: &mut Transaction,
        parent: T,
        index: usize,
    ) -> LoroResult<TreeID> {
        let inner = self.inner.try_attached_state()?;
        let parent: Option<TreeID> = parent.into();
        let target = TreeID::from_id(txn.next_id());

        match self.generate_position_at(&target, parent, index) {
            FractionalIndexGenResult::Ok(position) => {
                self.create_with_position(inner, txn, target, parent, index, position)
            }
            FractionalIndexGenResult::Rearrange(ids) => {
                for (i, (id, position)) in ids.into_iter().enumerate() {
                    if i == 0 {
                        self.create_with_position(inner, txn, id, parent, index, position)?;
                        continue;
                    }
                    self.mov_with_position(inner, txn, id, parent, index + i, position)?;
                }
                Ok(target)
            }
        }
    }

    pub fn mov<T: Into<Option<TreeID>>>(&self, target: TreeID, parent: T) -> LoroResult<()> {
        let parent = parent.into();
        match &self.inner {
            MaybeDetached::Detached(_) => {
                let mut index: usize = self.children_num(parent).unwrap_or(0);
                if self.is_parent(target, parent) {
                    index -= 1;
                }
                self.move_to(target, parent, index)
            }
            MaybeDetached::Attached(a) => {
                let mut index = self.children_num(parent).unwrap_or(0);
                if self.is_parent(target, parent) {
                    index -= 1;
                }
                a.with_txn(|txn| self.mov_with_txn(txn, target, parent, index))
            }
        }
    }

    pub fn mov_after(&self, target: TreeID, other: TreeID) -> LoroResult<()> {
        let parent: Option<TreeID> = self
            .get_node_parent(&other)
            .ok_or(LoroTreeError::TreeNodeNotExist(other))?;
        let mut index = self.get_index_by_tree_id(&other).unwrap() + 1;
        if self.is_parent(target, parent) && self.get_index_by_tree_id(&target).unwrap() < index {
            index -= 1;
        }
        self.move_to(target, parent, index)
    }

    pub fn mov_before(&self, target: TreeID, other: TreeID) -> LoroResult<()> {
        let parent = self
            .get_node_parent(&other)
            .ok_or(LoroTreeError::TreeNodeNotExist(other))?;
        let mut index = self.get_index_by_tree_id(&other).unwrap();
        if self.is_parent(target, parent)
            && index > 1
            && self.get_index_by_tree_id(&target).unwrap() < index
        {
            index -= 1;
        }
        self.move_to(target, parent, index)
    }

    pub fn move_to<T: Into<Option<TreeID>>>(
        &self,
        target: TreeID,
        parent: T,
        index: usize,
    ) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                t.value.mov(target, parent.into(), index)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.mov_with_txn(txn, target, parent, index))
            }
        }
    }

    pub fn mov_with_txn<T: Into<Option<TreeID>>>(
        &self,
        txn: &mut Transaction,
        target: TreeID,
        parent: T,
        index: usize,
    ) -> LoroResult<()> {
        let parent = parent.into();
        let inner = self.inner.try_attached_state()?;
        let mut children_len = self.children_num(parent).unwrap_or(0);
        let mut already_in_parent = false;
        // check the input is valid
        if self.is_parent(target, parent) {
            // If the position after moving is same as the current position , do nothing
            if let Some(current_index) = self.get_index_by_tree_id(&target) {
                if current_index == index {
                    return Ok(());
                }
                // move out first, we cannot delete the position here
                // If throw error, the tree will be in a inconsistent state
                children_len -= 1;
                already_in_parent = true;
            }
        };
        if index > children_len {
            return Err(LoroTreeError::IndexOutOfBound {
                len: children_len,
                index,
            }
            .into());
        }
        if already_in_parent {
            self.delete_position(parent, target);
        }

        match self.generate_position_at(&target, parent, index) {
            FractionalIndexGenResult::Ok(position) => {
                self.mov_with_position(inner, txn, target, parent, index, position)
            }
            FractionalIndexGenResult::Rearrange(ids) => {
                for (i, (id, position)) in ids.into_iter().enumerate() {
                    self.mov_with_position(inner, txn, id, parent, index + i, position)?;
                }
                Ok(())
            }
        }
    }

    fn create_with_position(
        &self,
        inner: &BasicHandler,
        txn: &mut Transaction,
        tree_id: TreeID,
        parent: Option<TreeID>,
        index: usize,
        position: FractionalIndex,
    ) -> LoroResult<TreeID> {
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target: tree_id,
                parent,
                position: Some(position.clone()),
            }),
            EventHint::Tree(TreeDiffItem {
                target: tree_id,
                action: TreeExternalDiff::Create {
                    parent,
                    index,
                    position,
                },
            }),
            &inner.state,
        )?;
        Ok(tree_id)
    }

    fn mov_with_position(
        &self,
        inner: &BasicHandler,
        txn: &mut Transaction,
        target: TreeID,
        parent: Option<TreeID>,
        index: usize,
        position: FractionalIndex,
    ) -> LoroResult<()> {
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target,
                parent,
                position: Some(position.clone()),
            }),
            EventHint::Tree(TreeDiffItem {
                target,
                action: TreeExternalDiff::Move {
                    parent,
                    index,
                    position,
                    old_parent: self
                        .get_node_parent(&target)
                        .map(TreeParentId::from)
                        .unwrap_or(TreeParentId::Unexist),
                    old_index: self.get_index_by_tree_id(&target).unwrap_or(0),
                },
            }),
            &inner.state,
        )
    }

    pub fn get_meta(&self, target: TreeID) -> LoroResult<MapHandler> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.try_lock().unwrap();
                d.value
                    .map
                    .get(&target)
                    .cloned()
                    .ok_or(LoroTreeError::TreeNodeNotExist(target).into())
            }
            MaybeDetached::Attached(a) => {
                if !self.contains(target) {
                    return Err(LoroTreeError::TreeNodeNotExist(target).into());
                }
                let map_container_id = target.associated_meta_container();
                let handler = create_handler(a, map_container_id);
                Ok(handler.into_map().unwrap())
            }
        }
    }

    /// Get the parent of the node, if the node is deleted or does not exist, return None
    pub fn get_node_parent(&self, target: &TreeID) -> Option<Option<TreeID>> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_parent(target)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                match a.parent(target) {
                    TreeParentId::Root => Some(None),
                    TreeParentId::Node(parent) => Some(Some(parent)),
                    TreeParentId::Deleted | TreeParentId::Unexist => None,
                }
            }),
        }
    }

    // TODO: iterator
    pub fn children(&self, parent: Option<TreeID>) -> Vec<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_children(parent).unwrap()
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.get_children(&TreeParentId::from(parent))
                    .unwrap()
                    .collect()
            }),
        }
    }

    pub fn children_num(&self, parent: Option<TreeID>) -> Option<usize> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.children_num(parent)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.children_num(&TreeParentId::from(parent))
            }),
        }
    }

    pub fn contains(&self, target: TreeID) -> bool {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.map.contains_key(&target)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.contains(target)
            }),
        }
    }

    pub fn get_child_at(&self, parent: Option<TreeID>, index: usize) -> Option<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_id_by_index(&parent, index)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.get_id_by_index(&TreeParentId::from(parent), index)
            }),
        }
    }

    pub fn is_parent(&self, target: TreeID, parent: Option<TreeID>) -> bool {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.is_parent(target, parent)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.is_parent(&TreeParentId::from(parent), &target)
            }),
        }
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.map.keys().cloned().collect()
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.nodes()
            }),
        }
    }

    pub fn roots(&self) -> Vec<TreeID> {
        self.children(None)
    }

    #[allow(non_snake_case)]
    pub fn __internal__next_tree_id(&self) -> TreeID {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.try_lock().unwrap();
                TreeID::new(PeerID::MAX, d.value.next_counter)
            }
            MaybeDetached::Attached(a) => a
                .with_txn(|txn| Ok(TreeID::from_id(txn.next_id())))
                .unwrap(),
        }
    }

    fn generate_position_at(
        &self,
        target: &TreeID,
        parent: Option<TreeID>,
        index: usize,
    ) -> FractionalIndexGenResult {
        let MaybeDetached::Attached(a) = &self.inner else {
            unreachable!()
        };
        a.with_state(|state| {
            let a = state.as_tree_state_mut().unwrap();
            a.generate_position_at(target, &TreeParentId::from(parent), index)
        })
    }

    /// Get the index of the target node in the parent node
    ///
    /// O(logN)
    pub fn get_index_by_tree_id(&self, target: &TreeID) -> Option<usize> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_index_by_tree_id(target)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.get_index_by_tree_id(target)
            }),
        }
    }

    pub fn get_position_by_tree_id(&self, target: &TreeID) -> Option<FractionalIndex> {
        match &self.inner {
            MaybeDetached::Detached(_) => unreachable!(),
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.get_position(target)
            }),
        }
    }

    fn delete_position(&self, parent: Option<TreeID>, target: TreeID) {
        let MaybeDetached::Attached(a) = &self.inner else {
            unreachable!()
        };
        a.with_state(|state| {
            let a = state.as_tree_state_mut().unwrap();
            a.delete_position(&TreeParentId::from(parent), target)
        })
    }
}
