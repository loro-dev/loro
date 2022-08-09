use super::*;

impl<'a, T: Rle, A: RleTreeTrait<T>> LeafNode<'a, T, A> {
    #[inline]
    pub fn new(bump: &'a Bump, parent: NonNull<InternalNode<'a, T, A>>) -> Self {
        Self {
            bump,
            parent,
            children: FixedSizedVec::with_capacity(A::MAX_CHILDREN_NUM, bump),
            prev: None,
            next: None,
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
        ans.next = self.next;
        ans.prev = Some(NonNull::new(self).unwrap());
        self.next = Some(NonNull::new(&mut ans).unwrap());
        ans
    }

    pub fn push_child(&mut self, value: T) -> Result<(), Self> {
        if self.children.len() > 0 {
            let last = self.children.last_mut().unwrap();
            if last.is_mergable(&value, &()) {
                last.merge(&value, &());
                A::update_cache_leaf(self);
                return Ok(());
            }
        }

        if self.children.len() == A::MAX_CHILDREN_NUM {
            let mut ans = self._split();
            ans.push_child(value);
            A::update_cache_leaf(self);
            A::update_cache_leaf(&mut ans);
            return Err(ans);
        }

        self.children.push(value);
        A::update_cache_leaf(self);
        Ok(())
    }

    pub fn insert(&mut self, raw_index: A::Int, value: T) -> Result<(), Self> {
        if self.children.len() == 0 {
            self.children.push(value);
            return Ok(());
        }

        let (mut index, mut offset) = A::find_insert_pos_leaf(self, raw_index);
        let prev = if offset == 0 {
            Some(&mut self.children[index - 1])
        } else if offset == self.children[index].len() {
            index += 1;
            offset = 0;
            Some(&mut self.children[index - 1])
        } else {
            None
        };

        if let Some(prev) = prev {
            // clean cut, should no split
            if prev.is_mergable(&value, &()) {
                prev.merge(&value, &());
                A::update_cache_leaf(self);
                return Ok(());
            }

            if self.children.len() == A::MAX_CHILDREN_NUM {
                let mut ans = self._split();
                if index <= self.children.len() {
                    self.children.insert(index, value);
                } else {
                    ans.children.insert(index - self.children.len(), value);
                }

                A::update_cache_leaf(self);
                A::update_cache_leaf(&mut ans);
                return Err(ans);
            } else {
                self.children.insert(index, value);
                A::update_cache_leaf(self);
                return Ok(());
            }
        }

        // need to split child
        let a = self.children[index].slice(0, offset);
        let b = self.children[index].slice(offset, self.children[index].len());
        self.children[index] = a;

        if self.children.len() == A::MAX_CHILDREN_NUM {
            let mut ans = self._split();
            if index < self.children.len() {
                self.children.insert(index + 1, b);
            } else {
                ans.children.insert(index - self.children.len() + 1, b);
            }

            A::update_cache_leaf(self);
            A::update_cache_leaf(&mut ans);
            return Err(ans);
        }

        self.children.insert(index + 1, value);
        A::update_cache_leaf(self);
        Ok(())
    }
}
