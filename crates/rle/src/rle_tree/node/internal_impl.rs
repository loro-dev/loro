use std::{
    collections::HashSet,
    fmt::{Debug, Error, Formatter},
    mem::transmute,
    ops::DerefMut,
};

use fxhash::FxHashSet;
use smallvec::SmallVec;

use crate::{
    rle_tree::{
        arena::VecTrait,
        node::utils::distribute,
        tree_trait::{FindPosResult, InsertResult, Position},
    },
    small_set::SmallSet,
};

use super::*;

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    #[inline(always)]
    pub fn new(bump: &'a A::Arena, parent: Option<NonNull<Self>>) -> Self {
        Self {
            bump,
            parent,
            children: <A::Arena as Arena>::Vec::with_capacity_in(A::MAX_CHILDREN_NUM, bump),
            cache: Default::default(),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    /// return result need to update cache
    #[inline]
    fn _split(
        &mut self,
    ) -> (
        A::CacheInParent,
        <A::Arena as Arena>::Boxed<'a, Node<'a, T, A>>,
    ) {
        let mut ans = self
            .bump
            .allocate(Node::Internal(Self::new(self.bump, self.parent)));
        let inner = ans.as_internal_mut().unwrap();
        let update = self._balance(inner);
        (update, ans)
    }

    /// return result need to update cache
    #[inline]
    fn _balance(&mut self, other: &mut Self) -> A::CacheInParent {
        let keep_num = (self.children.len() + other.children.len()) / 2;
        debug_assert!(keep_num >= A::MIN_CHILDREN_NUM);
        let mut update = A::CacheInParent::default();
        for mut child in self.children.drain(keep_num..) {
            update += child.parent_cache;
            child.node.set_parent(other.into());
            other.children.push(child);
        }

        debug_assert!(self.children.len() >= A::MIN_CHILDREN_NUM);
        debug_assert!(other.children.len() >= A::MIN_CHILDREN_NUM);
        -update
    }

    #[inline(always)]
    pub fn children(&self) -> &[Child<'a, T, A>] {
        &self.children
    }

    #[cfg(test)]
    pub(crate) fn _check_child_parent(&self) {
        for child in self.children.iter() {
            child.node.get_self_index().unwrap();
            match child.node.deref() {
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
        let mut is_leaf_children = false;
        for child in self.children.iter_mut() {
            assert_eq!(child.parent_cache, child.node.cache().into());
            match child.node.deref_mut() {
                Node::Internal(node) => {
                    assert!(!is_leaf_children);
                    node.check();
                }
                Node::Leaf(node) => {
                    is_leaf_children = true;
                    node.check();
                }
            }
        }

        if is_leaf_children {
            let mut last: Option<NonNull<LeafNode<'a, T, A>>> = None;
            for child in self.children.iter() {
                if let Some(ref mut last) = last {
                    assert_eq!(
                        child.node.as_leaf().unwrap().prev,
                        Some(*last),
                        "{:#?}",
                        &self
                    );
                    // SAFETY: for test
                    unsafe {
                        assert_eq!(
                            last.as_ref().next,
                            Some(child.node.as_leaf().unwrap().into())
                        )
                    };
                    *last = child.node.as_leaf().unwrap().into();
                } else {
                    last = Some(child.node.as_leaf().unwrap().into());
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
            if let Some(child) = child.node.as_internal() {
                child.check_balance_recursively();
            }
        }
    }

    fn check_children_parent_link(&mut self) {
        let self_ptr = self as *const _;
        for child in self.children.iter_mut() {
            match child.node.deref_mut() {
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
    #[allow(clippy::type_complexity)]
    fn _delete<F>(
        &mut self,
        from: Option<A::Int>,
        to: Option<A::Int>,
        visited: &mut SmallVec<[(usize, NonNull<Node<'a, T, A>>); 8]>,
        depth: usize,
        notify: &mut F,
    ) -> Result<
        A::CacheInParent,
        (
            A::CacheInParent,
            <A::Arena as Arena>::Boxed<'a, Node<'a, T, A>>,
        ),
    >
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            return Ok(Default::default());
        }

        debug_log::group!("_delete");
        let (direct_delete_start, to_del_start_offset) =
            from.map_or((0, None), |x| self._delete_start(x));
        let (direct_delete_end, to_del_end_offset) =
            to.map_or((self.children.len(), None), |x| self._delete_end(x));
        let deleted_len = direct_delete_end as isize - direct_delete_start as isize;
        // TODO: maybe we can simplify this insertions logic
        let mut insertions: SmallVec<[(usize, <A::Arena as Arena>::Boxed<'a, Node<T, A>>); 2]> =
            smallvec::smallvec![];
        let mut update = A::CacheInParent::default();
        {
            // handle deletions at the end point
            let mut handled = false;
            if let (Some(del_from), Some(del_to)) = (to_del_start_offset, to_del_end_offset) {
                if direct_delete_start - 1 == direct_delete_end {
                    debug_log::debug_log!("Meet");
                    // start and end are at the same child
                    let child = &mut self.children[direct_delete_end];
                    match child.node.deref_mut() {
                        Node::Internal(node) => {
                            match node._delete(
                                Some(del_from),
                                Some(del_to),
                                visited,
                                depth + 1,
                                notify,
                            ) {
                                Ok(hint) => {
                                    child.parent_cache += hint;
                                    update += hint;
                                }
                                Err((hint, new)) => {
                                    child.parent_cache += hint;
                                    insertions.push((direct_delete_end + 1, new));
                                    update += hint;
                                }
                            }
                        }
                        Node::Leaf(node) => {
                            match node.delete(Some(del_from), Some(del_to), notify) {
                                Ok(hint) => {
                                    child.parent_cache += hint;
                                    update += hint;
                                }
                                Err((hint, new)) => {
                                    child.parent_cache += hint;
                                    insertions.push((direct_delete_end + 1, new));
                                    update += hint;
                                }
                            }
                        }
                    }
                    visited.push((
                        depth,
                        self.children[direct_delete_end].node.deref_mut().into(),
                    ));
                    handled = true;
                }
            }

            if !handled {
                if let Some(del_from) = to_del_start_offset {
                    // handle deletions at the start
                    visited.push((
                        depth,
                        NonNull::new(&mut *self.children[direct_delete_start - 1].node).unwrap(),
                    ));
                    let child = &mut self.children[direct_delete_start - 1];
                    match child.node.deref_mut() {
                        Node::Internal(node) => {
                            match node._delete(Some(del_from), None, visited, depth + 1, notify) {
                                Ok(hint) => {
                                    child.parent_cache += hint;
                                    update += hint;
                                }
                                Err((hint, new)) => {
                                    child.parent_cache += hint;
                                    // even if we panic here it can still work
                                    insertions.push((direct_delete_start, new));
                                    update += hint;
                                }
                            }
                        }
                        Node::Leaf(node) => {
                            match node.delete(Some(del_from), None, notify) {
                                Ok(hint) => {
                                    child.parent_cache += hint;
                                    update += hint;
                                }
                                Err((hint, new)) => {
                                    child.parent_cache += hint;
                                    // even if we panic here it can still work
                                    insertions.push((direct_delete_start, new));
                                    update += hint;
                                }
                            }
                        }
                    }
                }
                if let Some(del_to) = to_del_end_offset {
                    // handle deletions at the end
                    visited.push((
                        depth,
                        NonNull::new(&mut *self.children[direct_delete_end].node).unwrap(),
                    ));
                    let child = &mut self.children[direct_delete_end];
                    debug_log::group!("End");
                    match child.node.deref_mut() {
                        Node::Internal(node) => {
                            match node._delete(None, Some(del_to), visited, depth + 1, notify) {
                                Ok(hint) => {
                                    child.parent_cache += hint;
                                    debug_log::debug_log!(
                                        "ok, \ncache = {:?}\nactual = {:?}",
                                        child.parent_cache,
                                        child.node.cache()
                                    );
                                    update += hint;
                                }
                                Err((hint, new)) => {
                                    child.parent_cache += hint;
                                    debug_log::debug_log!(
                                        "ok, \ncache = {:?}\nactual = {:?}",
                                        child.parent_cache,
                                        child.node.cache()
                                    );
                                    // even if we panic here it can still work
                                    insertions.push((direct_delete_end + 1, new));
                                    update += hint;
                                }
                            }
                        }
                        Node::Leaf(node) => {
                            match node.delete(None, Some(del_to), notify) {
                                Ok(hint) => {
                                    child.parent_cache += hint;
                                    debug_log::debug_log!(
                                        "ok, \ncache = {:?}\nactual = {:?}",
                                        child.parent_cache,
                                        child.node.cache()
                                    );
                                    update += hint;
                                }
                                Err((hint, new)) => {
                                    child.parent_cache += hint;
                                    debug_log::debug_log!(
                                        "err, \ncache = {:?}\nactual = {:?}",
                                        child.parent_cache,
                                        child.node.cache()
                                    );
                                    // even if we panic here it can still work
                                    insertions.push((direct_delete_end + 1, new));
                                    update += hint;
                                }
                            }
                        }
                    }
                    debug_log::group_end!();
                }
            }
        }

        if deleted_len > 0 {
            debug_log::debug_log!("Range");
            update += self.drain_children(direct_delete_start, direct_delete_end);
        }

        insertions.sort_by_key(|x| -(x.0 as isize));
        let mut result = Ok(());
        for mut insertion in insertions {
            if insertion.0 >= direct_delete_end && deleted_len > 0 {
                debug_log::debug_log!("S");
                insertion.0 -= deleted_len as usize;
            }

            debug_log::debug_log!("m");
            match self._insert_with_split(insertion.0, insertion.1) {
                Ok(hint) => {
                    update += hint;
                }
                Err((hint, new)) => {
                    assert!(result.is_ok());
                    result = Err(new);
                    update += hint;
                }
            }
        }

        debug_log::debug_dbg!(&update);
        A::update_cache_internal(self, Some(update));
        if let Err(new) = &mut result {
            A::update_cache_internal(new.as_internal_mut().unwrap(), None);
        }

        debug_log::group_end!();

        match result {
            Ok(_) => Ok(update),
            Err(new) => Err((update, new)),
        }
    }

    #[inline(always)]
    pub fn drain_children(
        &mut self,
        direct_delete_start: usize,
        direct_delete_end: usize,
    ) -> A::CacheInParent {
        self.connect_leaf(direct_delete_start, direct_delete_end - 1);
        let mut update = A::CacheInParent::default();
        for item in self.children.drain(direct_delete_start..direct_delete_end) {
            update += item.parent_cache;
        }

        -update
    }

    pub(crate) fn apply_updates(
        &mut self,
        mut updates: Vec<(usize, A::CacheInParent, Vec<ArenaBoxedNode<'a, T, A>>)>,
    ) -> Result<A::CacheInParent, (A::CacheInParent, Vec<ArenaBoxedNode<'a, T, A>>)> {
        let mut update_sum = A::CacheInParent::default();
        for (index, update, _) in updates.iter().filter(|x| x.2.is_empty()) {
            update_sum += *update;
            self.children[*index].parent_cache += *update;
        }

        updates.retain(|x| !x.2.is_empty());
        if updates.is_empty() {
            A::update_cache_internal(self, None);
            return Ok(update_sum);
        }

        updates.sort_by_key(|x| x.0);
        let mut new_children: Vec<<A::Arena as Arena>::Boxed<'a, Node<'a, T, A>>> =
            Vec::with_capacity(A::MAX_CHILDREN_NUM);
        let mut saved_end = 0;
        for (index, _, replace) in updates {
            for child in self.children.drain(0..index + 1 - saved_end) {
                new_children.push(child.node);
            }

            for element in replace {
                new_children.push(element);
            }

            saved_end = index + 1;
        }

        for child in self.children.drain(..) {
            new_children.push(child.node);
        }

        let self_ptr: NonNull<_> = self.into();
        let result = if new_children.len() <= A::MAX_CHILDREN_NUM {
            for mut child in new_children {
                child.set_parent(self_ptr);
                self.children.push(Child::from(child));
            }

            Ok(A::update_cache_internal(self, None))
        } else {
            let children_nums =
                distribute(new_children.len(), A::MIN_CHILDREN_NUM, A::MAX_CHILDREN_NUM);
            let mut index = 0;
            for mut child in new_children.drain(0..children_nums[index]) {
                child.set_parent(self_ptr);
                self.children.push(Child::from(child));
            }

            index += 1;
            let update = A::update_cache_internal(self, None);
            let mut ans_vec = Vec::new();
            while !new_children.is_empty() {
                let mut new_internal_node = self
                    .bump
                    .allocate(Node::Internal(InternalNode::new(self.bump, self.parent)));
                let new_internal = new_internal_node.as_internal_mut().unwrap();
                for mut child in new_children.drain(..children_nums[index]) {
                    child.set_parent(new_internal.into());
                    new_internal.children.push(Child::from(child));
                }

                index += 1;
                A::update_cache_internal(new_internal, None);
                ans_vec.push(new_internal_node);
            }

            Err((update, ans_vec))
        };

        if result.is_err() && self.is_root() {
            #[allow(clippy::unnecessary_unwrap)]
            let (update, new_vec) = result.unwrap_err();
            {
                // create level
                let mut origin_root = self.bump.allocate(Node::Internal(InternalNode::new(
                    self.bump,
                    Some(self.into()),
                )));
                let origin_root_internal = origin_root.as_internal_mut().unwrap();
                std::mem::swap(&mut self.children, &mut origin_root_internal.children);
                let ptr = origin_root_internal.into();
                for child in origin_root_internal.children.iter_mut() {
                    child.node.set_parent(ptr);
                }

                A::update_cache_internal(origin_root_internal, None);
                self.children.push(Child::from(origin_root));
            }

            let ptr = self.into();
            for mut new_node in new_vec {
                new_node.set_parent(ptr);
                self.children.push(Child::from(new_node));
            }

            Ok(A::update_cache_internal(self, None))
        } else {
            result
        }
    }

    /// connect [prev leaf of left] with [next leaf of right]
    fn connect_leaf(&mut self, left_index: usize, right_index: usize) {
        let prev = self.children[left_index]
            .node
            .get_first_leaf()
            .and_then(|x| x.prev);
        let next = self.children[right_index]
            .node
            .get_last_leaf()
            .and_then(|x| x.next);
        // SAFETY: rle_tree is single threaded and we have the exclusive ref
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

    #[inline(always)]
    fn _delete_start(&mut self, from: A::Int) -> (usize, Option<A::Int>) {
        let from = A::find_pos_internal(self, from);
        if from.pos == Position::Start || from.pos == Position::Before {
            (from.child_index, None)
        } else {
            (from.child_index + 1, Some(from.offset))
        }
    }

    #[inline(always)]
    fn _delete_end(&mut self, to: A::Int) -> (usize, Option<A::Int>) {
        let to = A::find_pos_internal(self, to);
        if to.pos == Position::End || to.pos == Position::After {
            (to.child_index + 1, None)
        } else {
            (to.child_index, Some(to.offset))
        }
    }

    #[inline]
    pub fn insert<F>(&mut self, index: A::Int, value: T, notify: &mut F) -> InsertResult<'a, T, A>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        match self._insert(index, value, notify) {
            Ok(update) => Ok(A::update_cache_internal(self, Some(update))),
            Err((update, mut new)) => {
                A::update_cache_internal(self, Some(update));
                A::update_cache_internal(new.as_internal_mut().unwrap(), None);
                if self.is_root() {
                    self._create_level(new);
                    Ok(Default::default())
                } else {
                    Err((update, new))
                }
            }
        }
    }

    /// root node function. assume self and new's caches are up-to-date
    fn _create_level(&mut self, mut new: <A::Arena as Arena>::Boxed<'a, Node<'a, T, A>>) {
        debug_assert!(self.is_root());
        let mut left = self
            .bump
            .allocate(Node::Internal(InternalNode::new(self.bump, None)));
        let left_inner = left.as_internal_mut().unwrap();
        std::mem::swap(left_inner, self);
        let left_ptr = left_inner.into();
        for child in left_inner.children.iter_mut() {
            child.node.set_parent(left_ptr);
        }

        left_inner.parent = Some(NonNull::new(self).unwrap());
        new.as_internal_mut().unwrap().parent = Some(self.into());
        self.children.push(Child::from(left));
        self.children.push(Child::from(new));
        // TODO: perf
        A::update_cache_internal(self, None);
    }

    fn _insert<F>(&mut self, index: A::Int, value: T, notify: &mut F) -> InsertResult<'a, T, A>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            debug_assert!(self.is_root());
            let ptr = NonNull::new(self as *mut _).unwrap();
            self.children.push(Child {
                parent_cache: Default::default(),
                node: Node::new_leaf(self.bump, ptr),
            });
        }

        let FindPosResult {
            child_index,
            offset: relative_idx,
            ..
        } = A::find_pos_internal(self, index);
        let child = &mut self.children[child_index];
        let result = match child.node.deref_mut() {
            Node::Internal(child) => child.insert(relative_idx, value, notify),
            Node::Leaf(child) => child.insert(relative_idx, value, notify),
        };

        let mut update: A::CacheInParent;
        child.parent_cache = child.node.cache().into();
        match result {
            Ok(hint) => {
                update = hint;
            }
            Err((hint, new)) => {
                update = hint;
                match self._insert_with_split(child_index + 1, new) {
                    Ok(hint) => {
                        update += hint;
                    }
                    Err((hint, new)) => {
                        update += hint;
                        return Err((update, new));
                    }
                }
            }
        }

        Ok(update)
    }

    #[inline]
    pub(crate) fn insert_at_pos(
        &mut self,
        index: usize,
        value: <A::Arena as Arena>::Boxed<'a, Node<'a, T, A>>,
    ) -> InsertResult<'a, T, A> {
        for child in self.children.iter() {
            debug_assert_eq!(child.parent_cache, child.node.cache().into());
        }
        let result = self._insert_with_split(index, value);
        match result {
            // TODO: Perf, neet to be aware of node may not be in valid state in the beginning of this fn
            Ok(_) => Ok(A::update_cache_internal(self, None)),
            Err((_, mut new)) => {
                // TODO: Perf, neet to be aware of node may not be in valid state in the beginning of this fn
                let hint = A::update_cache_internal(self, None);
                A::update_cache_internal(new.as_internal_mut().unwrap(), None);
                if self.is_root() {
                    self._create_level(new);
                    Ok(Default::default())
                } else {
                    Err((hint, new))
                }
            }
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    /// this can only invoke from root
    #[inline]
    pub(crate) fn delete<F>(&mut self, start: Option<A::Int>, end: Option<A::Int>, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        debug_log::group!("DELETE");
        debug_assert!(self.is_root());
        let mut old_zipper = SmallVec::new();
        match self._delete(start, end, &mut old_zipper, 1, notify) {
            Ok(_) => {}
            Err((_, new)) => {
                self._create_level(new);
            }
        };

        let removed = self._root_shrink_levels_if_one_child();

        // filter the same
        let mut visited: SmallSet<NonNull<_>, 12> = SmallSet::new();
        let mut should_skip: SmallSet<NonNull<_>, 12> = SmallSet::new();
        let mut depth_to_node: SmallVec<[SmallVec<[NonNull<_>; 2]>; 10]> = smallvec::smallvec![];
        use heapless::binary_heap::{BinaryHeap, Max};
        let mut zipper: BinaryHeap<(isize, NonNull<Node<'a, T, A>>), Max, 32> = Default::default();
        for v in old_zipper.into_iter().filter_map(|(i, ptr)| {
            if removed.contains(&ptr) {
                return None;
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
        }) {
            zipper.push(v).unwrap();
        }
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

            let to_delete: bool;
            if let Some(index) = node.get_self_index() {
                let parent = node.parent_mut().unwrap();
                debug_log::debug_log!("borrow/merge sibling");
                let Some(should_delete) = parent.balance_child(index, notify) else {
                    continue;
                };
                to_delete = should_delete;
            } else {
                continue;
            }

            if to_delete {
                should_skip.insert(node_ptr);
                if let Some(parent) = depth_to_node.get((-reverse_depth - 1) as usize).as_ref() {
                    for ptr in parent.iter() {
                        zipper.push((reverse_depth + 1, *ptr)).unwrap();
                    }
                }
                {
                    // remove node
                    let this = node;
                    if let Some(leaf) = this.as_leaf_mut() {
                        let next = leaf.next;
                        let prev = leaf.prev;
                        if let Some(mut next) = next {
                            // SAFETY: it is safe here
                            unsafe { next.as_mut() }.prev = prev;
                        }
                        if let Some(mut prev) = prev {
                            // SAFETY: it is safe here
                            unsafe { prev.as_mut() }.next = next;
                        }
                    }

                    let index = this.get_self_index().unwrap();
                    let parent = this.parent_mut().unwrap();
                    for _ in parent.children.drain(index..index + 1) {}
                };
            }
        }

        self._root_shrink_levels_if_one_child();
        debug_log::group_end!();
    }

    fn _root_shrink_levels_if_one_child(&mut self) -> FxHashSet<NonNull<Node<'a, T, A>>> {
        let mut ans: HashSet<_, _> = FxHashSet::default();
        while self.children.len() == 1 && self.children[0].node.as_internal().is_some() {
            debug_log::debug_log!("Shrink");
            let mut child = self.children.pop().unwrap();
            ans.insert(child.node.deref().into());
            let child_ptr = child.node.as_internal_mut().unwrap();
            std::mem::swap(&mut *child_ptr, self);
            self.parent = None;
            let ptr = self.into();
            // TODO: extract reset parent?
            for child in self.children.iter_mut() {
                child.node.set_parent(ptr);
            }

            child_ptr.parent = None;
            child_ptr.children.clear();
        }

        ans
    }

    #[inline(always)]
    fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    fn _insert_with_split(
        &mut self,
        child_index: usize,
        mut new: <A::Arena as Arena>::Boxed<'a, Node<'a, T, A>>,
    ) -> InsertResult<'a, T, A> {
        if self.children.len() == A::MAX_CHILDREN_NUM {
            let (mut update, mut ans) = self._split();
            if child_index < self.children.len() {
                new.set_parent(self.into());
                update += new.cache().into();
                self.children.insert(child_index, Child::from(new));
            } else {
                new.set_parent((&mut *ans.as_internal_mut().unwrap()).into());
                ans.as_internal_mut()
                    .unwrap()
                    .children
                    .insert(child_index - self.children.len(), Child::from(new));
            }

            Err((update, ans))
        } else {
            new.set_parent(self.into());
            let update = (new.cache()).into();
            self.children.insert(child_index, Child::from(new));
            Ok(update)
        }
    }

    fn balance_child<F>(&mut self, index: usize, notify: &mut F) -> Option<bool>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.len() == 1 {
            return None;
        }

        let (sibling_index, either) = if index > 0 {
            (index - 1, Either::Left)
        } else {
            (index + 1, Either::Right)
        };
        let (node, sibling) = arref::array_mut_ref!(&mut self.children, [index, sibling_index]);
        if node.node.children_num() + sibling.node.children_num() <= A::MAX_CHILDREN_NUM {
            node.node
                .merge_to_sibling(&mut sibling.node, either, notify);
            sibling.parent_cache = sibling.node.cache().into();
            Some(true)
        } else {
            node.node
                .borrow_from_sibling(&mut sibling.node, either, notify);
            node.parent_cache = node.node.cache().into();
            sibling.parent_cache = sibling.node.cache().into();
            Some(false)
        }
    }

    pub fn get_index_in_parent(&self) -> Option<usize> {
        let parent = self.parent.unwrap();
        // SAFETY: we know parent must be valid
        let parent = unsafe { parent.as_ref() };
        parent
            .children
            .iter()
            .position(|child| std::ptr::eq(child.node.as_internal().unwrap(), self))
    }

    #[inline(always)]
    pub(crate) fn update_cache(&mut self, hint: Option<A::CacheInParent>) {
        // TODO: perf
        for child in self.children.iter_mut() {
            child.parent_cache = child.node.cache().into();
        }
        A::update_cache_internal(self, hint);
    }

    #[inline(always)]
    pub fn parent(&self) -> &Option<NonNull<InternalNode<'a, T, A>>> {
        &self.parent
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
