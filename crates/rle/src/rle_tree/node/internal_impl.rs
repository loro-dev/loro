use crate::HasLength;

use super::*;

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

        let (mut child_index, mut child_new_insert_idx) = A::find_insert_pos_internal(self, index);
        let child = &mut self.children[child_index];
        let new = match child {
            Node::Internal(child) => {
                if let Err(new) = child.insert(child_new_insert_idx, value) {
                    let new = Node::Internal(new);
                    Some(new)
                } else {
                    None
                }
            }
            Node::Leaf(child) => {
                if let Err(new) = child.insert(child_new_insert_idx, value) {
                    let new = Node::Leaf(new);
                    Some(new)
                } else {
                    None
                }
            }
        };

        if let Some(new) = new {
            if self.children.len() == A::MAX_CHILDREN_NUM {
                let mut ans = self._split();
                if child_index < self.children.len() {
                    self.children.insert(child_index + 1, new);
                } else {
                    ans.children
                        .insert(child_index - self.children.len() + 1, new);
                }

                return Err(ans);
            }

            self.children.insert(child_index + 1, new);
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
