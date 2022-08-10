use crate::{rle_tree::tree_trait::Position, HasLength};

use super::{node_trait::NodeTrait, *};

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    pub fn new(bump: &'a Bump, parent: Option<NonNull<Self>>) -> Self {
        Self {
            bump,
            parent,
            children: FixedSizedVec::with_capacity(A::MAX_CHILDREN_NUM, bump),
            cache: Default::default(),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    #[inline]
    fn _split(&mut self) -> BumpBox<'a, Self> {
        let mut ans = BumpBox::new_in(Self::new(self.bump, self.parent), self.bump);
        for child in self
            .children
            .drain(self.children.len() - A::MIN_CHILDREN_NUM..self.children.len())
        {
            ans.children.push(child);
        }

        ans
    }

    #[inline]
    pub fn children(&self) -> &[Node<'a, T, A>] {
        &self.children
    }

    #[cfg(test)]
    pub(crate) fn check(&self) {
        for child in self.children.iter() {
            match child {
                Node::Internal(node) => node.check(),
                Node::Leaf(node) => node.check(),
            }
        }
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

    pub fn insert(&mut self, index: A::Int, value: T) -> Result<(), BumpBox<'a, Self>> {
        match self._insert(index, value) {
            Ok(_) => {
                A::update_cache_internal(self);
                Ok(())
            }
            Err(mut new) => {
                A::update_cache_internal(self);
                A::update_cache_internal(&mut new);
                Err(new)
            }
        }
    }

    fn _insert(&mut self, index: A::Int, value: T) -> Result<(), BumpBox<'a, Self>> {
        if self.children.len() == 0 {
            debug_assert!(self.parent.is_none());
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

impl<'a, T: Rle, A: RleTreeTrait<T>> NodeTrait<'a, T, A> for InternalNode<'a, T, A> {
    type Child = Node<'a, T, A>;
    fn to_node(node: BumpBox<'a, Self>) -> Node<'a, T, A> {
        Node::Internal(node)
    }

    // TODO: simplify this func?
    fn delete(
        &mut self,
        from: Option<A::Int>,
        to: Option<A::Int>,
    ) -> Result<(), BumpBox<'a, Self>> {
        let (del_start, to_del_from) = from.map_or((0, None), |x| self._delete_start(x));
        let (del_end, to_del_to) = to.map_or((self.children.len(), None), |x| self._delete_end(x));
        let mut result = Ok(());
        {
            // handle edge removing
            let mut handled = false;
            if let (Some(del_from), Some(del_to)) = (to_del_from, to_del_to) {
                if del_start - 1 == del_end {
                    match &mut self.children[del_end] {
                        Node::Internal(node) => {
                            if let Err(new) = node.delete(Some(del_from), Some(del_to)) {
                                result = self._insert_with_split(del_end + 1, Node::Internal(new));
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(Some(del_from), Some(del_to)) {
                                result = self._insert_with_split(del_end + 1, Node::Leaf(new));
                            }
                        }
                    }
                    handled = true;
                }
            }

            if !handled {
                if let Some(del_from) = to_del_from {
                    match &mut self.children[del_start - 1] {
                        Node::Internal(node) => {
                            if let Err(new) = node.delete(Some(del_from), None) {
                                result =
                                    self._insert_with_split(del_start, NodeTrait::to_node(new));
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(Some(del_from), None) {
                                result = self._insert_with_split(del_start, NodeTrait::to_node(new))
                            }
                        }
                    }
                }
                if let Some(del_to) = to_del_to {
                    match &mut self.children[del_end] {
                        Node::Internal(node) => {
                            if let Err(new) = node.delete(None, Some(del_to)) {
                                result =
                                    self._insert_with_split(del_end + 1, NodeTrait::to_node(new))
                            }
                        }
                        Node::Leaf(node) => {
                            if let Err(new) = node.delete(None, Some(del_to)) {
                                result =
                                    self._insert_with_split(del_end + 1, NodeTrait::to_node(new))
                            }
                        }
                    }
                }
            }
        }

        if del_start < del_end {
            for _ in self.children.drain(del_start..del_end) {}
        }

        A::update_cache_internal(self);
        result
    }

    fn _insert_with_split(
        &mut self,
        child_index: usize,
        new: Node<'a, T, A>,
    ) -> Result<(), BumpBox<'a, Self>> {
        if self.children.len() == A::MAX_CHILDREN_NUM {
            let mut ans = self._split();
            if child_index < self.children.len() {
                self.children.insert(child_index, new);
            } else {
                ans.children.insert(child_index - self.children.len(), new);
            }

            return Err(ans);
        }

        self.children.insert(child_index, new);
        Ok(())
    }
}
