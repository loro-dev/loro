use super::*;

impl<'a, T: Rle, A: RleTreeTrait<T>> LeafNode<'a, T, A> {
    pub fn new(bump: &'a Bump, parent: &'a InternalNode<'a, T, A>) -> Self {
        Self {
            bump,
            parent,
            children: BumpVec::with_capacity_in(A::max_children(), bump),
            prev: None,
            next: None,
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }
}
