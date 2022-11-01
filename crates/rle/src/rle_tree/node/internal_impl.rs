use std::{
    collections::{BinaryHeap, HashSet},
    fmt::{Debug, Error, Formatter},
};

use fxhash::FxHashSet;
use smallvec::SmallVec;

use crate::{
    rle_tree::{
        node::utils::distribute,
        tree_trait::{FindPosResult, Position},
    },
    small_set::SmallSet,
};

use super::*;

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    pub fn new(bump: &'a Bump, parent: Option<NonNull<Self>>) -> Self {
        Self {
            bump,
            parent,
            children: BumpVec::with_capacity_in(A::MAX_CHILDREN_NUM, bump),
            cache: Default::default(),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    /// return result need to update cache
    #[inline]
    fn _split(&mut self) -> &'a mut Node<'a, T, A> {
        let ans = self
            .bump
            .alloc(Node::Internal(Self::new(self.bump, self.parent)));
        let inner = ans.as_internal_mut().unwrap();
        self._balance(inner);
        ans
    }

    /// return result need to update cache
    #[inline]
    fn _balance(&mut self, other: &mut Self) {
        let keep_num = (self.children.len() + other.children.len()) / 2;
        debug_assert!(keep_num >= A::MIN_CHILDREN_NUM);
        for child in self.children.drain(keep_num..) {
            child.set_parent(other.into());
            other.children.push(child);
        }

        debug_assert!(self.children.len() >= A::MIN_CHILDREN_NUM);
        debug_assert!(other.children.len() >= A::MIN_CHILDREN_NUM);
    }

    #[inline]
    pub fn children(&self) -> &[&'a mut Node<'a, T, A>] {
        &self.children
    }

    #[cfg(test)]
    pub(crate) fn _check_child_parent(&self) {
        for child in self.children.iter() {
            child.get_self_index().unwrap();
            match child {
                Node::Internal(node) => {
                    assert!(std::ptr::eq(node.parent.unwrap().as_ptr(), self));
                    node._check_child_parent();
                }
                Node::Leaf(node) => {
                    assert!(std::ptr::eq(node.parent.as_ptr(), self));
                }
            }
        }
    }

    pub(crate) fn check(&mut self) {
        self.check_balance();

        self.check_children_parent_link();
        for child in self.children.iter_mut() {
            match child {
                Node::Internal(node) => {
                    node.check();
                }
                Node::Leaf(node) => {
                    node.check();
                }
            }
        }
        A::check_cache_internal(self);
    }

    fn check_balance(&mut self) {
        if !self.is_root() {
            assert!(
                self.children.len() >= A::MIN_CHILDREN_NUM,
                "children.len() = {}",
                self.children.len()
            );
        }
        assert!(
            self.children.len() <= A::MAX_CHILDREN_NUM,
            "children.len() = {}",
            self.children.len()
        );
    }

    fn check_balance_recursively(&self) {
        if !self.is_root() {
            assert!(
                self.children.len() >= A::MIN_CHILDREN_NUM,
                "children.len() = {}",
                self.children.len()
            );
        }
        assert!(
            self.children.len() <= A::MAX_CHILDREN_NUM,
            "children.len() = {}",
            self.children.len()
        );

        for child in self.children.iter() {
            if let Some(child) = child.as_internal() {
                child.check_balance_recursively();
            }
        }
    }

    fn check_children_parent_link(&mut self) {
        let self_ptr = self as *const _;
        for child in self.children.iter_mut() {
            match child {
                Node::Internal(node) => {
                    assert!(std::ptr::eq(node.parent.unwrap().as_ptr(), self_ptr));
                }
                Node::Leaf(node) => {
                    assert!(std::ptr::eq(node.parent.as_ptr(), self_ptr));
                }
            }
        }
    }

    // TODO: simplify this func?
    fn _delete<F>(
        &mut self,
        from: Option<A::Int>,
        to: Option<A::Int>,
        visited: &mut SmallVec<[(usize, NonNull<Node<'a, T, A>>); 8]>,
        depth: usize,
        notify: &mut F,
    ) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            return Ok(());
        }

        let (direct_delete_start, to_del_start_offset) =
            from.map_or((0, None), |x| self._delete_start(x));
        let (direct_delete_end, to_del_end_offset) =
            to.map_or((self.children.len(), None), |x| self._delete_end(x));
        let deleted_len = direct_delete_end as isize - direct_delete_start as isize;
        // TODO: maybe we can simplify this insertions logic
        let mut insertions: SmallVec<[(usize, &mut Node<T, A>); 2]> = smallvec::smallvec![];
        {
            // handle removing at the end point
            let mut handled = false;
            if let (Some(del_from), Some(del_to)) = (to_del_start_offset, to_del_end_offset) {
                if direct_delete_start - 1 == direct_delete_end {
                    visited.push((
                        depth,
                        NonNull::new(&mut *self.children[direct_delete_end]).unwrap(),
                    ));
                    match &mut self.children[direct_delete_end] {
                        Node::Internal(node) => {
                            if let Err(new) = node._delete(
                                Some(del_from),
                                Some(del_to),
                                visited,
                                depth + 1,
                                notify,
                            ) {
                                insertions.push((direct_delete_end + 1, new));
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(Some(del_from), Some(del_to), notify) {
                                insertions.push((direct_delete_end + 1, new));
                            }
                        }
                    }
                    handled = true;
                }
            }

            if !handled {
                if let Some(del_from) = to_del_start_offset {
                    visited.push((
                        depth,
                        NonNull::new(&mut *self.children[direct_delete_start - 1]).unwrap(),
                    ));
                    match &mut self.children[direct_delete_start - 1] {
                        Node::Internal(node) => {
                            if let Err(new) =
                                node._delete(Some(del_from), None, visited, depth + 1, notify)
                            {
                                // TODO: maybe even if we panic here it can still work
                                insertions.push((direct_delete_start, new));
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(Some(del_from), None, notify) {
                                // TODO: maybe even if we panic here it can still work
                                insertions.push((direct_delete_start, new));
                            }
                        }
                    }
                }
                if let Some(del_to) = to_del_end_offset {
                    visited.push((
                        depth,
                        NonNull::new(&mut *self.children[direct_delete_end]).unwrap(),
                    ));
                    match &mut self.children[direct_delete_end] {
                        Node::Internal(node) => {
                            if let Err(new) =
                                node._delete(None, Some(del_to), visited, depth + 1, notify)
                            {
                                // TODO: maybe even if we panic here it can still work
                                insertions.push((direct_delete_end + 1, new));
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(None, Some(del_to), notify) {
                                // TODO: maybe even if we panic here it can still work
                                insertions.push((direct_delete_end + 1, new));
                            }
                        }
                    }
                }
            }
        }

        if deleted_len > 0 {
            self.connect_leaf(direct_delete_start, direct_delete_end - 1);
            self.children.drain(direct_delete_start..direct_delete_end);
        }

        insertions.sort_by_key(|x| -(x.0 as isize));
        let mut result = Ok(());
        for mut insertion in insertions {
            if insertion.0 >= direct_delete_end && deleted_len > 0 {
                insertion.0 -= deleted_len as usize;
            }

            if let Err(data) = self._insert_with_split(insertion.0, insertion.1) {
                assert!(result.is_ok());
                result = Err(data);
            }
        }

        A::update_cache_internal(self);
        if let Err(new) = &mut result {
            A::update_cache_internal(new.as_internal_mut().unwrap());
        }

        result
    }

    pub(crate) fn apply_updates(
        &mut self,
        mut updates: Vec<(usize, Vec<&'a mut Node<'a, T, A>>)>,
    ) -> Result<(), Vec<&'a mut Node<'a, T, A>>> {
        if updates.is_empty() {
            A::update_cache_internal(self);
            return Ok(());
        }

        updates.sort_by_key(|x| x.0);
        let mut new_children: Vec<&'a mut Node<'a, T, A>> = Vec::with_capacity(A::MAX_CHILDREN_NUM);
        let mut self_children = std::mem::replace(
            &mut self.children,
            BumpVec::with_capacity_in(A::MAX_CHILDREN_NUM, self.bump),
        );
        let mut saved_end = 0;
        for (index, replace) in updates {
            for child in self_children.drain(0..index + 1 - saved_end) {
                new_children.push(child);
            }

            for element in replace {
                new_children.push(element);
            }

            saved_end = index + 1;
        }

        for child in self_children.drain(..) {
            new_children.push(child);
        }

        let self_ptr: NonNull<_> = self.into();
        let result = if new_children.len() <= A::MAX_CHILDREN_NUM {
            for child in new_children {
                child.set_parent(self_ptr);
                self.children.push(child);
            }

            A::update_cache_internal(self);
            Ok(())
        } else {
            let children_nums =
                distribute(new_children.len(), A::MIN_CHILDREN_NUM, A::MAX_CHILDREN_NUM);
            let mut index = 0;
            for child in new_children.drain(0..children_nums[index]) {
                child.set_parent(self_ptr);
                self.children.push(child);
            }

            index += 1;
            A::update_cache_internal(self);
            let mut ans_vec = Vec::new();
            while !new_children.is_empty() {
                let new_internal_node = self
                    .bump
                    .alloc(Node::Internal(InternalNode::new(self.bump, self.parent)));
                let new_internal = new_internal_node.as_internal_mut().unwrap();
                for child in new_children.drain(..children_nums[index]) {
                    child.set_parent(new_internal.into());
                    new_internal.children.push(child);
                }

                index += 1;
                A::update_cache_internal(new_internal);
                ans_vec.push(new_internal_node);
            }

            Err(ans_vec)
        };

        if result.is_err() && self.is_root() {
            #[allow(clippy::unnecessary_unwrap)]
            let new_vec = result.unwrap_err();
            {
                // create level
                let origin_root = self.bump.alloc(Node::Internal(InternalNode::new(
                    self.bump,
                    Some(self.into()),
                )));
                let origin_root_internal = origin_root.as_internal_mut().unwrap();
                std::mem::swap(&mut self.children, &mut origin_root_internal.children);
                let ptr = origin_root_internal.into();
                for child in origin_root_internal.children.iter_mut() {
                    child.set_parent(ptr);
                }

                A::update_cache_internal(origin_root_internal);
                self.children.push(origin_root);
            }

            let ptr = self.into();
            for new_node in new_vec {
                new_node.set_parent(ptr);
                self.children.push(new_node);
            }

            A::update_cache_internal(self);
            Ok(())
        } else {
            result
        }
    }

    /// connect [prev leaf of left] with [next leaf of right]
    fn connect_leaf(&mut self, left_index: usize, right_index: usize) {
        let prev = self.children[left_index]
            .get_first_leaf()
            .and_then(|x| x.prev);
        let next = self.children[right_index]
            .get_last_leaf()
            .and_then(|x| x.next);
        // SAFETY: rle_tree is single threaded
        unsafe {
            if let Some(mut prev) = prev {
                let prev = prev.as_mut();
                prev.next = next;
            }
            if let Some(mut next) = next {
                let next = next.as_mut();
                next.prev = prev;
            }
        }
    }

    fn _delete_start(&mut self, from: A::Int) -> (usize, Option<A::Int>) {
        let from = A::find_pos_internal(self, from);
        if from.pos == Position::Start || from.pos == Position::Before {
            (from.child_index, None)
        } else {
            (from.child_index + 1, Some(from.offset))
        }
    }

    fn _delete_end(&mut self, to: A::Int) -> (usize, Option<A::Int>) {
        let to = A::find_pos_internal(self, to);
        if to.pos == Position::End || to.pos == Position::After {
            (to.child_index + 1, None)
        } else {
            (to.child_index, Some(to.offset))
        }
    }

    pub fn insert<F>(
        &mut self,
        index: A::Int,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        match self._insert(index, value, notify) {
            Ok(_) => {
                A::update_cache_internal(self);
                Ok(())
            }
            Err(new) => {
                A::update_cache_internal(self);
                A::update_cache_internal(new.as_internal_mut().unwrap());
                if self.is_root() {
                    self._create_level(new);
                    Ok(())
                } else {
                    Err(new)
                }
            }
        }
    }

    /// root node function. assume self and new's caches are up-to-date
    fn _create_level(&mut self, new: &'a mut Node<'a, T, A>) {
        debug_assert!(self.is_root());
        let left = self
            .bump
            .alloc(Node::Internal(InternalNode::new(self.bump, None)));
        let left_inner = left.as_internal_mut().unwrap();
        std::mem::swap(left_inner, self);
        let left_ptr = left_inner.into();
        for child in left_inner.children.iter_mut() {
            child.set_parent(left_ptr);
        }

        left_inner.parent = Some(NonNull::new(self).unwrap());
        new.as_internal_mut().unwrap().parent = Some(self.into());
        self.children.push(left);
        self.children.push(new);
        A::update_cache_internal(self);
    }

    fn _insert<F>(
        &mut self,
        index: A::Int,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            debug_assert!(self.is_root());
            let ptr = NonNull::new(self as *mut _).unwrap();
            self.children.push(Node::new_leaf(self.bump, ptr));
        }

        let FindPosResult {
            child_index,
            offset: relative_idx,
            ..
        } = A::find_pos_internal(self, index);
        let child = &mut self.children[child_index];
        let new = match child {
            Node::Internal(child) => child.insert(relative_idx, value, notify),
            Node::Leaf(child) => child.insert(relative_idx, value, notify),
        };

        if let Err(new) = new {
            self._insert_with_split(child_index + 1, new)?
        }

        Ok(())
    }

    pub(crate) fn insert_at_pos(
        &mut self,
        index: usize,
        value: &'a mut Node<'a, T, A>,
    ) -> Result<(), &'a mut Node<'a, T, A>> {
        let result = self._insert_with_split(index, value);
        match result {
            Ok(_) => {
                A::update_cache_internal(self);
                Ok(())
            }
            Err(new) => {
                A::update_cache_internal(self);
                A::update_cache_internal(new.as_internal_mut().unwrap());
                if self.is_root() {
                    self._create_level(new);
                    Ok(())
                } else {
                    Err(new)
                }
            }
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    /// this can only invoke from root
    /// TODO: need to speed this method up. maybe remove hashset here? use a miniset instead
    #[inline]
    pub(crate) fn delete<F>(&mut self, start: Option<A::Int>, end: Option<A::Int>, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        debug_assert!(self.is_root());
        let mut zipper = SmallVec::new();
        match self._delete(start, end, &mut zipper, 1, notify) {
            Ok(_) => {
                A::update_cache_internal(self);
            }
            Err(new) => {
                A::update_cache_internal(self);
                A::update_cache_internal(new.as_internal_mut().unwrap());
                self._create_level(new);
            }
        };

        let removed = self._root_shrink_levels_if_one_child();

        // filter the same
        let mut visited: SmallSet<NonNull<_>, 12> = SmallSet::new();
        let mut should_skip: SmallSet<NonNull<_>, 12> = SmallSet::new();
        let mut depth_to_node: SmallVec<[SmallVec<[NonNull<_>; 2]>; 10]> = smallvec::smallvec![];
        let mut zipper: BinaryHeap<(isize, NonNull<Node<'a, T, A>>)> = zipper
            .into_iter()
            .filter_map(|(i, mut ptr)| {
                // SAFETY: node_ptr points to a valid descendant of self
                let node: &mut Node<'a, T, A> = unsafe { ptr.as_mut() };
                if let Some(node) = node.as_internal() {
                    let in_ptr = node as *const InternalNode<'a, T, A>;
                    if removed.contains(&in_ptr) {
                        return None;
                    }
                }

                if visited.contains(&ptr) {
                    None
                } else {
                    while depth_to_node.len() <= i {
                        depth_to_node.push(SmallVec::new());
                    }
                    depth_to_node[i].push(ptr);
                    visited.insert(ptr);
                    Some((-(i as isize), ptr))
                }
            })
            .collect();
        // visit in depth order, top to down (depth 0..inf)
        while let Some((reverse_depth, mut node_ptr)) = zipper.pop() {
            if should_skip.contains(&node_ptr) {
                continue;
            }

            // SAFETY: node_ptr points to a valid descendant of self
            let node: &mut Node<'a, T, A> = unsafe { node_ptr.as_mut() };
            debug_assert!(node.children_num() <= A::MAX_CHILDREN_NUM);
            if node.children_num() >= A::MIN_CHILDREN_NUM {
                continue;
            }

            let mut to_delete: bool = false;
            if let Some((sibling, either)) = node.get_a_sibling() {
                // if has sibling, borrow or merge to it
                let sibling: &mut Node<'a, T, A> =
                        // SAFETY: node's sibling points to a valid descendant of self
                        unsafe { &mut *((sibling as *const _) as usize as *mut _) };
                if node.children_num() + sibling.children_num() <= A::MAX_CHILDREN_NUM {
                    node.merge_to_sibling(sibling, either, notify);
                    to_delete = true;
                } else {
                    node.borrow_from_sibling(sibling, either, notify);
                }
            } else {
                if node.parent().unwrap().is_root() {
                    continue;
                }

                unreachable!();
            }

            if to_delete {
                should_skip.insert(node_ptr);
                if let Some(parent) = depth_to_node.get((-reverse_depth - 1) as usize).as_ref() {
                    for ptr in parent.iter() {
                        zipper.push((reverse_depth + 1, *ptr));
                    }
                }
                node.remove();
            }
        }

        self._root_shrink_levels_if_one_child();
    }

    fn _root_shrink_levels_if_one_child(&mut self) -> FxHashSet<*const InternalNode<'a, T, A>> {
        let mut ans: HashSet<_, _> = FxHashSet::default();
        while self.children.len() == 1 && self.children[0].as_internal().is_some() {
            let child = self.children.pop().unwrap();
            let child_ptr = child.as_internal_mut().unwrap();
            std::mem::swap(&mut *child_ptr, self);
            self.parent = None;
            let ptr = self.into();
            // TODO: extract reset parent?
            for child in self.children.iter_mut() {
                child.set_parent(ptr);
            }

            child_ptr.parent = None;
            child_ptr.children.clear();
            ans.insert(&*child_ptr as *const _);
        }

        ans
    }

    #[inline]
    fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    fn _insert_with_split(
        &mut self,
        child_index: usize,
        new: &'a mut Node<'a, T, A>,
    ) -> Result<(), &'a mut Node<'a, T, A>> {
        if self.children.len() == A::MAX_CHILDREN_NUM {
            let ans = self._split();
            if child_index < self.children.len() {
                new.set_parent(self.into());
                self.children.insert(child_index, new);
            } else {
                new.set_parent((&mut *ans.as_internal_mut().unwrap()).into());
                ans.as_internal_mut()
                    .unwrap()
                    .children
                    .insert(child_index - self.children.len(), new);
            }

            Err(ans)
        } else {
            new.set_parent(self.into());
            self.children.insert(child_index, new);
            Ok(())
        }
    }

    pub(crate) fn get_index_in_parent(&self) -> Option<usize> {
        let parent = self.parent.unwrap();
        // SAFETY: we know parent must be valid
        let parent = unsafe { parent.as_ref() };
        parent
            .children
            .iter()
            .position(|child| std::ptr::eq(child.as_internal().unwrap(), self))
    }

    #[inline(always)]
    pub(crate) fn update_cache(&mut self) {
        A::update_cache_internal(self);
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Debug for InternalNode<'a, T, A> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        let mut debug_struct = f.debug_struct("InternalNode");
        debug_struct.field("children", &self.children);
        debug_struct.field("cache", &self.cache);
        debug_struct.field("children_num", &self.children.len());
        debug_struct.finish()
    }
}
