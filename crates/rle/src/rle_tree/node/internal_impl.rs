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
    fn _split(&mut self) -> Self {
        let mut ans = Self::new(self.bump, self.parent);
        for i in 0..A::MIN_CHILDREN_NUM {
            ans.children.push(self.children.pop().unwrap());
        }

        ans
    }

    pub fn insert(&mut self, index: A::Int, value: T) -> Result<(), Self> {
        if self.children.len() == 0 {
            debug_assert!(self.parent.is_none());
            let ptr = NonNull::new(self as *mut _).unwrap();
            self.children.push(Node::new_leaf(self.bump, ptr));
            return Ok(());
        }

        let insert_pos = A::find_insert_pos_internal(self, index);
        let child = &mut self.children[insert_pos];
        let new = match child {
            Node::Internal(child) => {
                if let Err(new) = child.insert(index, value) {
                    let new = Node::Internal(BumpBox::new_in(new, self.bump));
                    Some(new)
                } else {
                    None
                }
            }
            Node::Leaf(child) => {
                if let Err(new) = child.insert(index, value) {
                    let new = Node::Leaf(BumpBox::new_in(new, self.bump));
                    Some(new)
                } else {
                    None
                }
            }
        };

        if let Some(new) = new {
            if self.children.len() == A::MAX_CHILDREN_NUM {
                let mut ans = self._split();
                if insert_pos <= self.children.len() {
                    self.children.insert(insert_pos, new);
                } else {
                    ans.children.insert(insert_pos - self.children.len(), new);
                }

                A::update_cache_internal(self);
                A::update_cache_internal(&mut ans);
                return Err(ans);
            }

            self.children.insert(insert_pos, new);
            A::update_cache_internal(self);
        }

        Ok(())
    }
}
