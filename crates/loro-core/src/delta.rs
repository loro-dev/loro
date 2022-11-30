use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, RleVec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta<Value, Meta = ()> {
    vec: RleVec<[DeltaItem<Value, Meta>; 1]>,
}

#[derive(Debug, EnumAsInner, PartialEq, Eq, Clone)]
pub enum DeltaItem<Value, Meta> {
    Retain { len: usize, meta: Meta },
    Insert { value: Value, meta: Meta },
    Delete(usize),
}

impl<Value: HasLength, Meta> HasLength for DeltaItem<Value, Meta> {
    fn content_len(&self) -> usize {
        match self {
            DeltaItem::Retain { len, meta: _ } => *len,
            DeltaItem::Insert { value, meta: _ } => value.atom_len(),
            DeltaItem::Delete(len) => *len,
        }
    }
}

impl<Value, Meta> Mergable for DeltaItem<Value, Meta> {}

impl<Value: HasLength, Meta> Delta<Value, Meta> {
    pub fn new() -> Self {
        Self { vec: RleVec::new() }
    }

    pub fn retain_with_meta(&mut self, len: usize, meta: Meta) {
        self.vec.push(DeltaItem::Retain { len, meta });
    }

    pub fn insert_with_meta(&mut self, value: Value, meta: Meta) {
        self.vec.push(DeltaItem::Insert { value, meta });
    }

    pub fn delete(&mut self, len: usize) {
        self.vec.push(DeltaItem::Delete(len));
    }

    pub fn iter(&self) -> impl Iterator<Item = &DeltaItem<Value, Meta>> {
        self.vec.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut DeltaItem<Value, Meta>> {
        self.vec.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<Value: HasLength, Meta> Default for Delta<Value, Meta> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Value: HasLength, Meta: Default> Delta<Value, Meta> {
    pub fn retain(&mut self, len: usize) {
        if len == 0 {
            return;
        }

        self.vec.push(DeltaItem::Retain {
            len,
            meta: Default::default(),
        });
    }

    pub fn insert(&mut self, value: Value) {
        self.vec.push(DeltaItem::Insert {
            value,
            meta: Default::default(),
        });
    }
}
