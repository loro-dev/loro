use crate::Sliceable;

impl Sliceable for bool {
    fn slice(&self, _: usize, _: usize) -> Self {
        *self
    }
}
