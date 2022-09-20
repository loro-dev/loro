use crate::rle_tree::{
    cursor::SafeCursorMut,
    tree_trait::{FindPosResult, Position},
};
use std::fmt::{Debug, Error, Formatter};

use super::*;

impl<'bump, T: Rle, A: RleTreeTrait<T>> LeafNode<'bump, T, A> {
    #[inline]
    pub fn new(bump: &'bump Bump, parent: NonNull<InternalNode<'bump, T, A>>) -> Self {
        Self {
            bump,
            parent,
            children: BumpVec::with_capacity_in(A::MAX_CHILDREN_NUM, bump),
            prev: None,
            next: None,
            cache: Default::default(),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    #[inline]
    fn _split<F>(&mut self, notify: &mut F) -> &'bump mut Node<'bump, T, A>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        let ans = self
            .bump
            .alloc(Node::Leaf(Self::new(self.bump, self.parent)));
        let mut ans_inner = ans.as_leaf_mut().unwrap();
        let ans_ptr = ans_inner as _;
        for child in self
            .children
            .drain(self.children.len() - A::MIN_CHILDREN_NUM..self.children.len())
        {
            notify(child, ans_ptr);
            ans_inner.children.push(child);
        }

        ans_inner.next = self.next;
        ans_inner.prev = Some(NonNull::new(self).unwrap());
        if let Some(mut next) = self.next {
            // SAFETY: ans_inner is a valid pointer
            unsafe { next.as_mut().prev = Some(NonNull::new_unchecked(ans_inner)) };
        }
        self.next = Some(NonNull::new(&mut *ans_inner).unwrap());
        ans
    }

    #[inline]
    pub fn get_cursor<'tree>(&'tree self, pos: A::Int) -> SafeCursor<'tree, 'bump, T, A> {
        let result = A::find_pos_leaf(self, pos);
        SafeCursor::new(self.into(), result.child_index, result.offset, result.pos)
    }

    #[inline]
    pub fn get_cursor_mut<'b>(&'b mut self, pos: A::Int) -> SafeCursorMut<'b, 'bump, T, A> {
        let result = A::find_pos_leaf(self, pos);
        SafeCursorMut::new(self.into(), result.child_index, result.offset, result.pos)
    }

    pub fn push_child<F>(
        &mut self,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'bump mut Node<'bump, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        let self_ptr = self as *mut _;
        if !self.children.is_empty() {
            let last = self.children.last_mut().unwrap();
            if last.is_mergable(&value, &()) {
                last.merge(&value, &());
                notify(last, self_ptr);
                A::update_cache_leaf(self);
                return Ok(());
            }
        }

        if self.children.len() == A::MAX_CHILDREN_NUM {
            let ans = self._split(notify);
            let inner = ans.as_leaf_mut().unwrap();
            inner.push_child(value, notify).unwrap();
            A::update_cache_leaf(self);
            A::update_cache_leaf(inner);
            return Err(ans);
        }

        self.children.push(self.bump.alloc(value));
        notify(self.children[self.children.len() - 1], self_ptr);
        A::update_cache_leaf(self);
        Ok(())
    }

    pub(crate) fn check(&mut self) {
        assert!(self.children.len() <= A::MAX_CHILDREN_NUM);
        A::check_cache_leaf(self);
        if let Some(next) = self.next {
            // SAFETY: this is only for testing, and next must be a valid pointer
            let self_ptr = unsafe { next.as_ref().prev.unwrap().as_ptr() };
            assert!(std::ptr::eq(self, self_ptr));
        }
        if let Some(prev) = self.prev {
            // SAFETY: this is only for testing, and prev must be a valid pointer
            let self_ptr = unsafe { prev.as_ref().next.unwrap().as_ptr() };
            assert!(std::ptr::eq(self, self_ptr));
        }
    }

    fn _delete_start(&mut self, from: A::Int) -> (usize, Option<usize>) {
        let result = A::find_pos_leaf(self, from);
        match result.pos {
            Position::Start | Position::Before => (result.child_index, None),
            Position::Middle | Position::End | Position::After => {
                (result.child_index + 1, Some(result.offset))
            }
        }
    }

    fn _delete_end(&mut self, to: A::Int) -> (usize, Option<usize>) {
        let result = A::find_pos_leaf(self, to);
        match result.pos {
            Position::After | Position::End => (result.child_index + 1, None),
            Position::Start | Position::Middle | Position::Before => {
                (result.child_index, Some(result.offset))
            }
        }
    }

    pub fn is_deleted(&self) -> bool {
        // SAFETY: we used bumpalo here, so even if current node is deleted we
        unsafe {
            let mut node = self.parent.as_ref();
            if !node
                .children
                .iter()
                .any(|x| std::ptr::eq(x.as_leaf().unwrap(), self))
            {
                return true;
            }

            while let Some(parent) = node.parent {
                let parent = parent.as_ref();
                if !parent
                    .children()
                    .iter()
                    .any(|x| std::ptr::eq(x.as_internal().unwrap(), node))
                {
                    return true;
                }

                node = parent;
            }
        }

        false
    }

    pub fn insert<F>(
        &mut self,
        raw_index: A::Int,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'bump mut Node<'bump, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        match self._insert(raw_index, value, notify) {
            Ok(_) => {
                A::update_cache_leaf(self);
                Ok(())
            }
            Err(new) => {
                A::update_cache_leaf(self);
                A::update_cache_leaf(new.as_leaf_mut().unwrap());
                Err(new)
            }
        }
    }

    fn _insert<F>(
        &mut self,
        raw_index: A::Int,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'bump mut Node<'bump, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            notify(&value, self);
            self.children.push(self.bump.alloc(value));
            return Ok(());
        }

        let FindPosResult {
            mut child_index,
            mut offset,
            mut pos,
            ..
        } = A::find_pos_leaf(self, raw_index);
        let self_ptr = self as *mut _;
        let prev = {
            if (pos == Position::Start || pos == Position::Before) && child_index > 0 {
                Some(&mut self.children[child_index - 1])
            } else if pos == Position::After || pos == Position::End {
                child_index += 1;
                offset = 0;
                pos = Position::Start;
                Some(&mut self.children[child_index - 1])
            } else {
                None
            }
        };

        if let Some(prev) = prev {
            // clean cut, should no split
            if prev.is_mergable(&value, &()) {
                prev.merge(&value, &());
                notify(prev, self_ptr);
                return Ok(());
            }
        }

        let clean_cut = pos != Position::Middle;
        if clean_cut {
            return self._insert_with_split(child_index, value, notify);
        }

        // need to split child
        let a = self.children[child_index].slice(0, offset);
        let b = self.children[child_index].slice(offset, self.children[child_index].len());
        self.children[child_index] = self.bump.alloc(a);

        if self.children.len() >= A::MAX_CHILDREN_NUM - 1 {
            let next_node = self._split(notify);
            let next_leaf = next_node.as_leaf_mut().unwrap();
            if child_index < self.children.len() {
                notify(&value, self_ptr);
                notify(&b, self_ptr);
                self.children
                    .insert(child_index + 1, self.bump.alloc(value));
                self.children.insert(child_index + 2, self.bump.alloc(b));

                let last_child = self.children.pop().unwrap();
                notify(last_child, next_leaf);
                next_leaf.children.insert(0, last_child);
            } else {
                notify(&value, next_leaf);
                next_leaf.children.insert(
                    child_index - self.children.len() + 1,
                    self.bump.alloc(value),
                );
                notify(&b, next_leaf);
                next_leaf
                    .children
                    .insert(child_index - self.children.len() + 2, self.bump.alloc(b));
            }

            return Err(next_node);
        }

        notify(&b, self);
        notify(&value, self);
        self.children.insert(child_index + 1, self.bump.alloc(b));
        self.children
            .insert(child_index + 1, self.bump.alloc(value));
        Ok(())
    }

    #[inline]
    pub fn next(&self) -> Option<&Self> {
        // SAFETY: internal variant ensure prev and next are valid reference
        unsafe { self.next.map(|p| p.as_ref()) }
    }

    #[inline]
    pub fn prev(&self) -> Option<&Self> {
        // SAFETY: internal variant ensure prev and next are valid reference
        unsafe { self.prev.map(|p| p.as_ref()) }
    }

    #[inline]
    pub fn children(&self) -> &[&'bump mut T] {
        &self.children
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> LeafNode<'a, T, A> {
    /// Delete may cause the children num increase, because splitting may happen
    ///
    pub(crate) fn delete<F>(
        &mut self,
        start: Option<A::Int>,
        end: Option<A::Int>,
        notify: &mut F,
    ) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            return Ok(());
        }

        let (del_start, del_relative_from) = start.map_or((0, None), |x| self._delete_start(x));
        let (del_end, del_relative_to) =
            end.map_or((self.children.len(), None), |x| self._delete_end(x));
        let mut handled = false;
        let mut result = Ok(());
        if let (Some(del_relative_from), Some(del_relative_to)) =
            (del_relative_from, del_relative_to)
        {
            if del_start - 1 == del_end {
                let end = &mut self.children[del_end];
                let (left, right) = (
                    end.slice(0, del_relative_from),
                    end.slice(del_relative_to, end.len()),
                );

                *end = self.bump.alloc(left);
                result = self._insert_with_split(del_end + 1, right, notify);
                handled = true;
            }
        }

        if !handled {
            if let Some(del_relative_from) = del_relative_from {
                self.children[del_start - 1] = self
                    .bump
                    .alloc(self.children[del_start - 1].slice(0, del_relative_from));
            }
            if let Some(del_relative_to) = del_relative_to {
                let self_ptr = self as *mut _;
                let end = &mut self.children[del_end];
                *end = self.bump.alloc(end.slice(del_relative_to, end.len()));
                notify(end, self_ptr);
            }
        }

        if del_start < del_end {
            for _ in self.children.drain(del_start..del_end) {}
        }

        A::update_cache_leaf(self);
        if let Err(new) = &mut result {
            A::update_cache_leaf(new.as_leaf_mut().unwrap());
        }

        result
    }

    fn _insert_with_split<F>(
        &mut self,
        index: usize,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.len() == A::MAX_CHILDREN_NUM {
            let ans = self._split(notify);
            if index <= self.children.len() {
                notify(&value, self);
                self.children.insert(index, self.bump.alloc(value));
            } else {
                let leaf = ans.as_leaf_mut().unwrap();
                notify(&value, leaf);
                leaf.children
                    .insert(index - self.children.len(), self.bump.alloc(value));
            }

            Err(ans)
        } else {
            notify(&value, self);
            self.children.insert(index, self.bump.alloc(value));
            Ok(())
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Debug for LeafNode<'a, T, A> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        let mut debug_struct = f.debug_struct("LeafNode");
        debug_struct.field("children", &self.children);
        debug_struct.field("cache", &self.cache);
        debug_struct.field("children_num", &self.children.len());
        debug_struct.finish()
    }
}
