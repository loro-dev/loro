use crate::rle_tree::{
    cursor::SafeCursorMut,
    tree_trait::{FindPosResult, Position},
};
use std::fmt::{Debug, Error, Formatter};

use super::*;

impl<'a, T: Rle, A: RleTreeTrait<T>> LeafNode<'a, T, A> {
    #[inline]
    pub fn new(bump: &'a Bump, parent: NonNull<InternalNode<'a, T, A>>) -> Self {
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
    fn _split<F>(&mut self, mut notify: &mut F) -> &'a mut Node<'a, T, A>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        let ans = self
            .bump
            .alloc(Node::Leaf(Self::new(self.bump, self.parent)));
        let mut inner = ans.as_leaf_mut().unwrap();
        let ans_ptr = inner as _;
        for child in self
            .children
            .drain(self.children.len() - A::MIN_CHILDREN_NUM..self.children.len())
        {
            notify(child, ans_ptr);
            inner.children.push(child);
        }

        inner.next = self.next;
        inner.prev = Some(NonNull::new(self).unwrap());
        self.next = Some(NonNull::new(&mut *inner).unwrap());
        ans
    }

    #[inline]
    pub fn get_cursor<'b>(&'b self, pos: A::Int) -> SafeCursor<'a, 'b, T, A> {
        let index = A::find_pos_leaf(self, pos).child_index;
        SafeCursor::new(self.into(), index)
    }

    #[inline]
    pub fn get_cursor_mut<'b>(&'b mut self, pos: A::Int) -> SafeCursorMut<'a, 'b, T, A> {
        let index = A::find_pos_leaf(self, pos).child_index;
        SafeCursorMut::new(self.into(), index)
    }

    pub fn push_child<F>(&mut self, value: T, notify: &mut F) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if !self.children.is_empty() {
            let last = self.children.last_mut().unwrap();
            if last.is_mergable(&value, &()) {
                last.merge(&value, &());
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
        A::update_cache_leaf(self);
        Ok(())
    }

    pub(crate) fn check(&mut self) {
        assert!(self.children.len() <= A::MAX_CHILDREN_NUM);
        A::check_cache_leaf(self);
    }

    fn _delete_start(&mut self, from: A::Int) -> (usize, Option<usize>) {
        let result = A::find_pos_leaf(self, from);
        if result.pos == Position::Start {
            (result.child_index, None)
        } else {
            (result.child_index + 1, Some(result.offset))
        }
    }

    fn _delete_end(&mut self, to: A::Int) -> (usize, Option<usize>) {
        let result = A::find_pos_leaf(self, to);
        if result.pos == Position::End {
            (result.child_index + 1, None)
        } else {
            (result.child_index, Some(result.offset))
        }
    }

    pub fn insert<F>(
        &mut self,
        raw_index: A::Int,
        value: T,
        notify: &mut F,
    ) -> Result<(), &'a mut Node<'a, T, A>>
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
    ) -> Result<(), &'a mut Node<'a, T, A>>
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if self.children.is_empty() {
            self.children.push(self.bump.alloc(value));
            return Ok(());
        }

        let FindPosResult {
            child_index: mut index,
            mut offset,
            ..
        } = A::find_pos_leaf(self, raw_index);
        let prev = {
            if offset == 0 && index > 0 {
                Some(&mut self.children[index - 1])
            } else if offset == self.children[index].len() {
                index += 1;
                offset = 0;
                Some(&mut self.children[index - 1])
            } else {
                None
            }
        };

        if let Some(prev) = prev {
            // clean cut, should no split
            if prev.is_mergable(&value, &()) {
                prev.merge(&value, &());
                return Ok(());
            }
        }

        let clean_cut = offset == 0 || offset == self.children[index].len();
        if clean_cut {
            return self._insert_with_split(index, value, notify);
        }

        // need to split child
        let a = self.children[index].slice(0, offset);
        let b = self.children[index].slice(offset, self.children[index].len());
        self.children[index] = self.bump.alloc(a);

        if self.children.len() >= A::MAX_CHILDREN_NUM - 1 {
            let node = self._split(notify);
            let leaf = node.as_leaf_mut().unwrap();
            if index < self.children.len() {
                self.children.insert(index + 1, self.bump.alloc(value));
                self.children.insert(index + 2, self.bump.alloc(b));
                leaf.children.insert(0, self.children.pop().unwrap());
            } else {
                leaf.children
                    .insert(index - self.children.len() + 1, self.bump.alloc(value));
                leaf.children
                    .insert(index - self.children.len() + 2, self.bump.alloc(b));
            }

            return Err(node);
        }

        self.children.insert(index + 1, self.bump.alloc(b));
        self.children.insert(index + 1, self.bump.alloc(value));
        Ok(())
    }

    #[inline]
    pub fn next(&self) -> Option<&Self> {
        self.next.map(|p| unsafe { p.as_ref() })
    }

    #[inline]
    pub fn prev(&self) -> Option<&Self> {
        self.prev.map(|p| unsafe { p.as_ref() })
    }

    #[inline]
    pub fn children(&self) -> &[&'a mut T] {
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
                let end = &mut self.children[del_end];
                *end = self.bump.alloc(end.slice(del_relative_to, end.len()));
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
                self.children.insert(index, self.bump.alloc(value));
            } else {
                ans.as_leaf_mut()
                    .unwrap()
                    .children
                    .insert(index - self.children.len(), self.bump.alloc(value));
            }

            Err(ans)
        } else {
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
