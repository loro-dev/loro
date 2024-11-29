use generic_btree::{Cursor, LeafIndex};

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem, DeltaRope,
};

pub struct Iter<'a, V: DeltaValue, Attr: DeltaAttr> {
    delta: &'a DeltaRope<V, Attr>,
    cursor: Option<LeafIndex>,
    current: Option<DeltaItem<V, Attr>>,
}

impl<'a, V: DeltaValue, Attr: DeltaAttr> Iter<'a, V, Attr> {
    pub fn new(delta: &'a DeltaRope<V, Attr>) -> Self {
        let leaf = delta.tree.first_leaf();
        let mut current = None;
        if let Some(leaf) = leaf {
            current = delta.tree.get_elem(leaf).cloned();
        }

        Self {
            delta,
            cursor: leaf,
            current,
        }
    }

    pub fn peek(&self) -> Option<&'_ DeltaItem<V, Attr>> {
        self.current.as_ref()
    }

    pub fn peek_is_replace(&self) -> bool {
        self.peek().map(|x| x.is_replace()).unwrap_or(false)
    }

    pub fn peek_is_insert(&self) -> bool {
        self.peek().map(|x| x.is_insert()).unwrap_or(false)
    }

    pub fn peek_is_delete(&self) -> bool {
        self.peek().map(|x| x.is_delete()).unwrap_or(false)
    }

    pub fn peek_is_retain(&self) -> bool {
        self.peek().map(|x| x.is_retain()).unwrap_or(false)
    }

    pub fn peek_length(&self) -> usize {
        self.peek().map(|x| x.delta_len()).unwrap_or(usize::MAX)
    }

    pub fn peek_insert_length(&self) -> usize {
        self.peek()
            .map(|x| match x {
                DeltaItem::Retain { .. } => 0,
                DeltaItem::Replace { value, .. } => value.rle_len(),
            })
            .unwrap_or(0)
    }

    pub fn next_with(&mut self, mut len: usize) -> Result<(), usize> {
        while len > 0 {
            let Some(current) = self.current.as_mut() else {
                return Err(len);
            };

            if len >= current.delta_len() {
                len -= current.delta_len();
                self.cursor = self
                    .delta
                    .tree
                    .next_elem(Cursor {
                        leaf: self.cursor.unwrap(),
                        offset: 0,
                    })
                    .map(|x| x.leaf);
                if let Some(leaf) = self.cursor {
                    self.current = self.delta.tree.get_elem(leaf).cloned();
                } else {
                    self.current = None;
                }
            } else {
                match current {
                    DeltaItem::Retain {
                        len: retain,
                        attr: _,
                    } => {
                        *retain -= len;
                    }
                    DeltaItem::Replace {
                        value,
                        attr: _,
                        delete,
                    } => {
                        if value.rle_len() > 0 {
                            value.slice_(len..);
                        } else {
                            *delete -= len;
                        }
                    }
                }
                len = 0;
            }
        }

        Ok(())
    }

    /// Consume next `len` deletions in the current item
    pub(crate) fn next_with_del(&mut self, mut len: usize) -> Result<(), usize> {
        let Some(current) = self.current.as_mut() else {
            return Err(len);
        };

        match current {
            DeltaItem::Retain { .. } => return Err(len),
            DeltaItem::Replace { delete, .. } => {
                if *delete >= len {
                    *delete -= len;
                    len = 0;
                } else {
                    len -= *delete;
                    *delete = 0;
                }
            }
        }

        if current.delta_len() == 0 {
            self.next();
        }

        if len > 0 {
            Err(len)
        } else {
            Ok(())
        }
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> Iterator for Iter<'_, V, Attr> {
    type Item = DeltaItem<V, Attr>;

    fn next(&mut self) -> Option<Self::Item> {
        self.cursor = self
            .delta
            .tree
            .next_elem(Cursor {
                leaf: self.cursor.unwrap(),
                offset: 0,
            })
            .map(|x| x.leaf);
        let old_current = std::mem::take(&mut self.current);
        if let Some(c) = self.cursor {
            self.current = self.delta.tree.get_elem(c).cloned();
        } else {
            self.current = None;
        }
        old_current
    }
}
