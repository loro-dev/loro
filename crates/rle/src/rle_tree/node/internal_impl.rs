use std::fmt::{Debug, Error, Formatter};

use crate::{rle_tree::tree_trait::Position, HasLength};

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

    #[inline]
    fn _split(&mut self) -> &'a mut Self {
        let ans = self.bump.alloc(Self::new(self.bump, self.parent));
        let ans_ptr = NonNull::new(&mut *ans).unwrap();
        for mut child in self
            .children
            .drain(self.children.len() - A::MIN_CHILDREN_NUM..self.children.len())
        {
            child.set_parent(ans_ptr);
            ans.children.push(child);
        }

        ans
    }

    #[inline]
    pub fn children(&self) -> &[Node<'a, T, A>] {
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
        if !self.is_root() {
            assert!(
                self.children.len() >= A::MIN_CHILDREN_NUM,
                "children.len() = {}",
                self.children.len()
            );
            assert!(
                self.children.len() <= A::MAX_CHILDREN_NUM,
                "children.len() = {}",
                self.children.len()
            );
        }

        let self_ptr = self as *const _;
        for child in self.children.iter_mut() {
            match child {
                Node::Internal(node) => {
                    node.check();
                    assert!(std::ptr::eq(node.parent.unwrap().as_ptr(), self_ptr));
                }
                Node::Leaf(node) => {
                    node.check();
                    assert!(std::ptr::eq(node.parent.as_ptr(), self_ptr));
                }
            }
        }

        A::check_cache_internal(self);
    }

    // TODO: simplify this func?
    fn _delete(
        &mut self,
        from: Option<A::Int>,
        to: Option<A::Int>,
        visited: &mut Vec<(usize, NonNull<Node<'a, T, A>>)>,
        depth: usize,
    ) -> Result<(), &'a mut Self> {
        let (direct_delete_start, to_del_start_offset) =
            from.map_or((0, None), |x| self._delete_start(x));
        let (direct_delete_end, to_del_end_offset) =
            to.map_or((self.children.len(), None), |x| self._delete_end(x));
        let mut result = Ok(());
        {
            // handle edge removing
            let mut handled = false;
            if let (Some(del_from), Some(del_to)) = (to_del_start_offset, to_del_end_offset) {
                if direct_delete_start - 1 == direct_delete_end {
                    visited.push((
                        depth,
                        NonNull::new(&mut self.children[direct_delete_end]).unwrap(),
                    ));
                    match &mut self.children[direct_delete_end] {
                        Node::Internal(node) => {
                            if let Err(new) =
                                node._delete(Some(del_from), Some(del_to), visited, depth + 1)
                            {
                                result = self
                                    ._insert_with_split(direct_delete_end + 1, Node::Internal(new));
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(Some(del_from), Some(del_to)) {
                                result =
                                    self._insert_with_split(direct_delete_end + 1, Node::Leaf(new));
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
                        NonNull::new(&mut self.children[direct_delete_start - 1]).unwrap(),
                    ));
                    match &mut self.children[direct_delete_start - 1] {
                        Node::Internal(node) => {
                            if let Err(new) = node._delete(Some(del_from), None, visited, depth + 1)
                            {
                                result = self._insert_with_split(direct_delete_start, new.into());
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(Some(del_from), None) {
                                result = self._insert_with_split(direct_delete_start, new.into())
                            }
                        }
                    }
                }
                if let Some(del_to) = to_del_end_offset {
                    visited.push((
                        depth,
                        NonNull::new(&mut self.children[direct_delete_end]).unwrap(),
                    ));
                    match &mut self.children[direct_delete_end] {
                        Node::Internal(node) => {
                            if let Err(new) = node._delete(None, Some(del_to), visited, depth + 1) {
                                debug_assert!(result.is_ok());
                                result = self._insert_with_split(direct_delete_end + 1, new.into());
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(None, Some(del_to)) {
                                debug_assert!(result.is_ok());
                                result = self._insert_with_split(direct_delete_end + 1, new.into());
                            }
                        }
                    }
                }
            }
        }

        if direct_delete_start < direct_delete_end {
            self.children.drain(direct_delete_start..direct_delete_end);
        }

        A::update_cache_internal(self);
        if let Err(new) = &mut result {
            A::update_cache_internal(new);
        }

        result
    }

    fn _delete_start(&mut self, from: A::Int) -> (usize, Option<A::Int>) {
        let (index_from, relative_from, pos_from) = A::find_pos_internal(self, from);
        if pos_from == Position::Start {
            (index_from, None)
        } else {
            (index_from + 1, Some(relative_from))
        }
    }

    fn _delete_end(&mut self, to: A::Int) -> (usize, Option<A::Int>) {
        let (index_to, relative_to, pos_to) = A::find_pos_internal(self, to);
        if pos_to == Position::End {
            (index_to + 1, None)
        } else {
            (index_to, Some(relative_to))
        }
    }

    pub fn insert(&mut self, index: A::Int, value: T) -> Result<(), &'a mut Self> {
        match self._insert(index, value) {
            Ok(_) => {
                A::update_cache_internal(self);
                Ok(())
            }
            Err(new) => {
                A::update_cache_internal(self);
                A::update_cache_internal(new);
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
    fn _create_level(&mut self, mut new: &'a mut InternalNode<'a, T, A>) {
        debug_assert!(self.is_root());
        let mut left = self.bump.alloc(InternalNode::new(self.bump, None));
        std::mem::swap(&mut *left, self);
        let left_ptr = (&mut *left).into();
        for child in left.children.iter_mut() {
            child.set_parent(left_ptr);
        }

        left.parent = Some(NonNull::new(self).unwrap());
        new.parent = Some(NonNull::new(self).unwrap());
        self.children.push(left.into());
        self.children.push(new.into());
        A::update_cache_internal(self);
    }

    fn _insert(&mut self, index: A::Int, value: T) -> Result<(), &'a mut Self> {
        if self.children.is_empty() {
            debug_assert!(self.is_root());
            let ptr = NonNull::new(self as *mut _).unwrap();
            self.children.push(Node::new_leaf(self.bump, ptr));
        }

        let (child_index, relative_idx, _) = A::find_pos_internal(self, index);
        let child = &mut self.children[child_index];
        let new = match child {
            Node::Internal(child) => {
                if let Err(new) = child.insert(relative_idx, value) {
                    let new = Node::Internal(new);
                    Some(new)
                } else {
                    None
                }
            }
            Node::Leaf(child) => {
                if let Err(new) = child.insert(relative_idx, value) {
                    let new = Node::Leaf(new);
                    Some(new)
                } else {
                    None
                }
            }
        };

        if let Some(new) = new {
            if let Err(value) = self._insert_with_split(child_index + 1, new) {
                return Err(value);
            }
        }

        Ok(())
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> HasLength for InternalNode<'a, T, A> {
    #[inline]
    fn len(&self) -> usize {
        A::len_internal(self)
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    /// this can only invoke from root
    #[inline]
    pub(crate) fn delete(&mut self, start: Option<A::Int>, end: Option<A::Int>) {
        debug_assert!(self.is_root());
        let mut visited = Vec::new();
        match self._delete(start, end, &mut visited, 1) {
            Ok(_) => {
                A::update_cache_internal(self);
            }
            Err(new) => {
                A::update_cache_internal(self);
                A::update_cache_internal(new);
                self._create_level(new);
            }
        };

        let removed = self._root_shrink_level_if_only_1_child();

        // visit in depth order, top to down (depth 0..inf)
        visited.sort();
        for (_, mut node) in visited.into_iter() {
            let node = unsafe { node.as_mut() };
            if let Some(node) = node.as_internal() {
                let ptr = &**node as *const InternalNode<'a, T, A>;
                if removed.contains(&ptr) {
                    continue;
                }
            }

            debug_assert!(node.children_num() <= A::MAX_CHILDREN_NUM);
            if node.children_num() >= A::MIN_CHILDREN_NUM {
                continue;
            }

            let mut to_delete: bool = false;
            if let Some((sibling, either)) = node.get_a_sibling() {
                // if has sibling, borrow or merge to it
                let sibling: &mut Node<'a, T, A> =
                    unsafe { &mut *((sibling as *const _) as usize as *mut _) };
                if node.children_num() + sibling.children_num() <= A::MAX_CHILDREN_NUM {
                    node.merge_to_sibling(sibling, either);
                    to_delete = true;
                } else {
                    node.borrow_from_sibling(sibling, either);
                }
            } else {
                if node.parent().unwrap().is_root() {
                    continue;
                }

                dbg!(self);
                dbg!(node.parent());
                dbg!(node);
                unreachable!();
            }

            if to_delete {
                node.remove();
            }
        }

        self._root_shrink_level_if_only_1_child();
    }

    fn _root_shrink_level_if_only_1_child(&mut self) -> Vec<*const InternalNode<'a, T, A>> {
        let mut ans = Vec::new();
        while self.children.len() == 1 && self.children[0].as_internal().is_some() {
            let mut child = self.children.pop().unwrap();
            let child_ptr = child.as_internal_mut().unwrap();
            std::mem::swap(&mut **child_ptr, self);
            self.parent = None;
            let ptr = self.into();
            // TODO: extract reset parent?
            for child in self.children.iter_mut() {
                child.set_parent(ptr);
            }

            child_ptr.parent = None;
            child_ptr.children.clear();
            ans.push(&**child_ptr as *const _);
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
        mut new: Node<'a, T, A>,
    ) -> Result<(), &'a mut Self> {
        if self.children.len() == A::MAX_CHILDREN_NUM {
            let ans = self._split();
            if child_index < self.children.len() {
                new.set_parent(self.into());
                self.children.insert(child_index, new);
            } else {
                new.set_parent((&mut *ans).into());
                ans.children.insert(child_index - self.children.len(), new);
            }

            Err(ans)
        } else {
            new.set_parent(self.into());
            self.children.insert(child_index, new);
            Ok(())
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Debug for InternalNode<'a, T, A> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        let mut debug_struct = f.debug_struct("InternalNode");
        debug_struct.field("children", &self.children);
        debug_struct.field("cache", &self.cache);
        debug_struct.finish()
    }
}
