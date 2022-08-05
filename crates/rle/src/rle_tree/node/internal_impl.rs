use super::*;

impl<'a, T: Rle, A: RleTreeTrait<T>> InternalNode<'a, T, A> {
    pub fn new(bump: &'a Bump) -> Self {
        Self {
            bump,
            parent: None,
            children: BumpVec::with_capacity_in(A::max_children(), bump),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }
}
