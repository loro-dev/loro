use enum_as_inner::EnumAsInner;
use serde::Serialize;
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta<Value, Meta = ()> {
    pub(crate) vec: Vec<DeltaItem<Value, Meta>>,
}

impl<V: Serialize, M: Serialize> Serialize for Delta<V, M> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.vec.iter())
    }
}

#[derive(Debug, EnumAsInner, Clone, PartialEq, Eq, Serialize)]
pub enum DeltaItem<Value, Meta> {
    Retain { retain: usize, attributes: Meta },
    Insert { insert: Value, attributes: Meta },
    Delete { delete: usize, attributes: Meta },
}

#[derive(PartialEq, Eq)]
pub enum DeltaType {
    Retain,
    Insert,
    Delete,
}

/// The metadata of a DeltaItem
/// If empty metadata is used to override the older one, we will remove the metadata into None
pub trait Meta: Debug + Clone + PartialEq + Default {
    fn empty() -> Self {
        Self::default()
    }
    fn is_empty(&self) -> bool;

    /// this is used when composing two [DeltaItem]s with the same length
    fn compose(&mut self, other: &Self, type_pair: (DeltaType, DeltaType));
    #[allow(unused)]
    fn take(&mut self, other: &Self) -> Self {
        self.clone()
    }

    fn is_mergeable(&self, other: &Self) -> bool;
    /// This is used when we merge two [DeltaItem]s.
    /// And it's guaranteed that [Meta::is_mergeable] is true
    fn merge(&mut self, other: &Self);
}

impl Meta for () {
    fn empty() -> Self {}
    fn is_empty(&self) -> bool {
        true
    }

    fn compose(&mut self, _other: &Self, _type_pair: (DeltaType, DeltaType)) {}

    fn is_mergeable(&self, _other: &Self) -> bool {
        true
    }

    fn merge(&mut self, _other: &Self) {}
}

/// The value of [DeltaItem::Insert]
pub trait DeltaValue: Debug + Sized {
    /// the other will be merged into self
    fn value_extend(&mut self, other: Self) -> Result<(), Self>;
    /// takes the first number of `length` elements
    fn take(&mut self, length: usize) -> Self;
    /// the length of the value
    fn length(&self) -> usize;
}

impl<V: DeltaValue, M: Debug> DeltaValue for DeltaItem<V, M> {
    fn value_extend(&mut self, _other: Self) -> Result<(), Self> {
        unreachable!()
    }

    fn take(&mut self, _length: usize) -> Self {
        unreachable!()
    }

    fn length(&self) -> usize {
        match self {
            DeltaItem::Retain {
                retain: len,
                attributes: _,
            } => *len,
            DeltaItem::Insert {
                insert: value,
                attributes: _,
            } => value.length(),
            DeltaItem::Delete {
                delete: len,
                attributes: _,
            } => *len,
        }
    }
}

impl<Value: DeltaValue, M: Meta> DeltaItem<Value, M> {
    pub fn meta(&self) -> &M {
        match self {
            DeltaItem::Insert {
                attributes: meta, ..
            } => meta,
            DeltaItem::Retain {
                attributes: meta, ..
            } => meta,
            DeltaItem::Delete {
                delete: _,
                attributes: meta,
            } => meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut M {
        match self {
            DeltaItem::Insert {
                attributes: meta, ..
            } => meta,
            DeltaItem::Retain {
                attributes: meta, ..
            } => meta,
            DeltaItem::Delete {
                delete: _,
                attributes: meta,
            } => meta,
        }
    }

    pub fn set_meta(&mut self, meta: M) {
        match self {
            DeltaItem::Insert { attributes: m, .. } => *m = meta,
            DeltaItem::Retain { attributes: m, .. } => *m = meta,
            DeltaItem::Delete {
                delete: _,
                attributes: m,
            } => *m = meta,
        }
    }

    fn type_(&self) -> DeltaType {
        match self {
            DeltaItem::Insert { .. } => DeltaType::Insert,
            DeltaItem::Retain { .. } => DeltaType::Retain,
            DeltaItem::Delete { .. } => DeltaType::Delete,
        }
    }

    pub fn compose_meta(&mut self, other: &Self) {
        let type_pair = (self.type_(), other.type_());
        let meta = self.meta_mut();
        let other_meta = other.meta();
        Meta::compose(meta, other_meta, type_pair);
    }

    // change self-length to self.len()-length
    // and return the taken one.
    pub(crate) fn take(&mut self, length: usize) -> Self {
        match self {
            DeltaItem::Insert {
                insert: value,
                attributes: meta,
            } => {
                let v = value.take(length);
                Self::Insert {
                    insert: v,
                    attributes: meta.clone(),
                }
            }
            DeltaItem::Retain {
                retain: len,
                attributes: meta,
            } => {
                *len -= length;
                Self::Retain {
                    retain: length,
                    attributes: meta.clone(),
                }
            }
            DeltaItem::Delete {
                delete: len,
                attributes: _,
            } => {
                *len -= length;
                Self::Delete {
                    delete: length,
                    // meta may store utf16 length, this take will invalidate it
                    attributes: M::empty(),
                }
            }
        }
    }

    pub(crate) fn take_with_meta_ref(&mut self, length: usize, other_meta: &Self) -> Self {
        match self {
            DeltaItem::Insert {
                insert: value,
                attributes: meta,
            } => {
                let v = value.take(length);
                Self::Insert {
                    insert: v,
                    attributes: meta.take(other_meta.meta()),
                }
            }
            DeltaItem::Retain {
                retain: len,
                attributes: meta,
            } => {
                *len -= length;
                Self::Retain {
                    retain: length,
                    attributes: meta.take(other_meta.meta()),
                }
            }
            DeltaItem::Delete {
                delete: len,
                attributes: meta,
            } => {
                *len -= length;
                Self::Delete {
                    delete: length,
                    attributes: meta.take(other_meta.meta()),
                }
            }
        }
    }

    fn insert_inner(self) -> Value {
        match self {
            DeltaItem::Insert { insert: value, .. } => value,
            _ => unreachable!(),
        }
    }

    pub fn is_retain(&self) -> bool {
        matches!(self, Self::Retain { .. })
    }

    pub fn is_insert(&self) -> bool {
        matches!(self, Self::Insert { .. })
    }

    pub fn is_delete(&self) -> bool {
        matches!(self, Self::Delete { .. })
    }
}

pub struct DeltaIterator<V, M: Meta> {
    // The reversed Vec uses pop() to simulate getting the first element each time
    ops: Vec<DeltaItem<V, M>>,
}

impl<V: DeltaValue, M: Meta> DeltaIterator<V, M> {
    fn new(ops: Vec<DeltaItem<V, M>>) -> Self {
        Self { ops }
    }

    #[inline(always)]
    fn next<L: Into<Option<usize>>>(&mut self, len: L) -> DeltaItem<V, M> {
        self.next_impl(len.into())
    }

    fn next_impl(&mut self, len: Option<usize>) -> DeltaItem<V, M> {
        let length = len.unwrap_or(usize::MAX);
        let next_op = self.peek_mut();
        if next_op.is_none() {
            return DeltaItem::Retain {
                retain: usize::MAX,
                attributes: M::empty(),
            };
        }
        let op = next_op.unwrap();
        let op_length = op.length();
        if length < op_length {
            // a part of the peek op
            op.take(length)
        } else {
            self.take_peek().unwrap()
        }
    }

    fn next_with_ref(&mut self, len: usize, other: &DeltaItem<V, M>) -> DeltaItem<V, M> {
        let next_op = self.peek_mut();
        if next_op.is_none() {
            return DeltaItem::Retain {
                retain: other.length(),
                attributes: other.meta().clone(),
            };
        }
        let op = next_op.unwrap();
        let op_length = op.length();
        if len < op_length {
            // a part of the peek op
            op.take_with_meta_ref(len, other)
        } else {
            self.take_peek().unwrap()
        }
    }

    fn next_pair(&mut self, other: &mut Self) -> (DeltaItem<V, M>, DeltaItem<V, M>) {
        let self_len = self.peek_length();
        let other_len = other.peek_length();
        if self_len > other_len {
            let length = other_len;
            let other_op = other.next(None);
            debug_assert_eq!(other_op.length(), length);
            let this_op = self.next_with_ref(length, &other_op);
            (this_op, other_op)
        } else {
            let length = self_len;
            let this_op = self.next(None);
            debug_assert_eq!(this_op.length(), length);
            let other_op = other.next_with_ref(length, &this_op);
            (this_op, other_op)
        }
    }

    fn peek_mut(&mut self) -> Option<&mut DeltaItem<V, M>> {
        self.ops.last_mut()
    }

    fn take_peek(&mut self) -> Option<DeltaItem<V, M>> {
        self.ops.pop()
    }

    fn rest(mut self) -> Vec<DeltaItem<V, M>> {
        self.ops.reverse();
        self.ops
    }

    fn has_next(&self) -> bool {
        !self.ops.is_empty()
    }

    fn peek(&self) -> Option<&DeltaItem<V, M>> {
        self.ops.last()
    }

    fn peek_length(&self) -> usize {
        if let Some(op) = self.peek() {
            op.length()
        } else {
            usize::MAX
        }
    }

    fn peek_is_insert(&self) -> bool {
        if let Some(op) = self.peek() {
            op.is_insert()
        } else {
            false
        }
    }

    fn peek_is_delete(&self) -> bool {
        if let Some(op) = self.peek() {
            op.is_delete()
        } else {
            false
        }
    }
}

impl<Value: DeltaValue, M: Meta> Delta<Value, M> {
    pub fn new() -> Self {
        Self { vec: Vec::new() }
    }

    pub fn items(&self) -> &[DeltaItem<Value, M>] {
        &self.vec
    }

    pub fn inner(self) -> Vec<DeltaItem<Value, M>> {
        self.vec
    }

    pub fn retain_with_meta(mut self, len: usize, meta: M) -> Self {
        if len == 0 {
            // currently no meta for retain if len == 0
            return self;
        }

        self.push(DeltaItem::Retain {
            retain: len,
            attributes: meta,
        });
        self
    }

    pub fn insert_with_meta<V: Into<Value>>(mut self, value: V, meta: M) -> Self {
        self.push(DeltaItem::Insert {
            insert: value.into(),
            attributes: meta,
        });
        self
    }

    pub fn delete_with_meta(mut self, len: usize, meta: M) -> Self {
        self.push(DeltaItem::Delete {
            delete: len,
            attributes: meta,
        });
        self
    }

    pub fn delete(mut self, len: usize) -> Self {
        if len == 0 {
            return self;
        }
        self.push(DeltaItem::Delete {
            delete: len,
            attributes: M::empty(),
        });
        self
    }

    pub fn retain(mut self, len: usize) -> Self {
        if len == 0 {
            return self;
        }

        self.push(DeltaItem::Retain {
            retain: len,
            attributes: M::empty(),
        });
        self
    }

    pub fn insert<V: Into<Value>>(mut self, value: V) -> Self {
        self.push(DeltaItem::Insert {
            insert: value.into(),
            attributes: M::empty(),
        });
        self
    }

    // If the new_op is merged, return true
    pub fn push(&mut self, new_op: DeltaItem<Value, M>) -> bool {
        let mut index = self.vec.len();
        let last_op = self.vec.pop();
        if let Some(mut last_op) = last_op {
            if new_op.is_delete() && last_op.is_delete() {
                *last_op.as_delete_mut().unwrap().0 += new_op.length();
                self.vec.push(last_op);
                return true;
            }
            // Since it does not matter if we insert before or after deleting at the same index,
            // always prefer to insert first
            if last_op.is_delete() && new_op.is_insert() {
                index -= 1;
                let _last_op = self.vec.pop();
                self.vec.push(last_op);
                if let Some(_last_op_inner) = _last_op {
                    last_op = _last_op_inner;
                } else {
                    self.vec.insert(0, new_op);
                    return true;
                };
            }
            if last_op.meta().is_mergeable(new_op.meta()) {
                if new_op.is_insert() && last_op.is_insert() {
                    let value = last_op.as_insert_mut().unwrap().0;
                    let meta = new_op.meta().clone();
                    match value.value_extend(new_op.insert_inner()) {
                        Ok(_) => {
                            last_op.meta_mut().merge(&meta);
                            self.vec.insert(index - 1, last_op);
                            return true;
                        }
                        Err(inner) => {
                            self.vec.insert(index - 1, last_op);
                            self.vec.insert(
                                index,
                                DeltaItem::Insert {
                                    insert: inner,
                                    attributes: meta,
                                },
                            );
                            return false;
                        }
                    }
                } else if new_op.is_retain() && last_op.is_retain() {
                    *last_op.as_retain_mut().unwrap().0 += new_op.length();
                    last_op.meta_mut().merge(new_op.meta());
                    // self.vec.push(last_op);
                    self.vec.insert(index - 1, last_op);
                    return true;
                }
            }

            // self.vec.push(last_op);
            self.vec.insert(index - 1, last_op);
        }
        if index == self.vec.len() {
            self.vec.push(new_op);
            false
        } else {
            self.vec.insert(index, new_op);
            true
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &DeltaItem<Value, M>> {
        self.vec.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut DeltaItem<Value, M>> {
        self.vec.iter_mut()
    }

    pub fn into_op_iter(self) -> DeltaIterator<Value, M> {
        DeltaIterator::new(self.vec.into_iter().rev().collect())
    }

    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reference: [Quill Delta](https://github.com/quilljs/delta)
    // TODO: PERF use &mut self and &other
    pub fn compose(self, other: Delta<Value, M>) -> Delta<Value, M> {
        // debug_log::debug_dbg!(&self, &other);
        let mut this_iter = self.into_op_iter();
        let mut other_iter = other.into_op_iter();
        let mut ops = vec![];
        let first_other = other_iter.peek();
        if let Some(first_other) = first_other {
            // if other.delta starts with retain, we insert corresponding number of inserts from self.delta
            if first_other.is_retain() && first_other.meta().is_empty() {
                let mut first_left = first_other.length();
                let mut first_this = this_iter.peek();
                while let Some(first_this_inner) = first_this {
                    if first_this_inner.is_insert() && first_this_inner.length() <= first_left {
                        first_left -= first_this_inner.length();
                        let mut op = this_iter.next(None);
                        op.compose_meta(first_other);
                        ops.push(op);
                        first_this = this_iter.peek();
                    } else {
                        break;
                    }
                }
                if first_other.length() - first_left > 0 {
                    other_iter.next(first_other.length() - first_left);
                }
            }
        }
        let mut delta = Delta { vec: ops };
        while this_iter.has_next() || other_iter.has_next() {
            if other_iter.peek_is_insert() {
                // nothing to compose here
                delta.push(other_iter.next(None));
            } else if this_iter.peek_is_delete() {
                // nothing to compose here
                delta.push(this_iter.next(None));
            } else {
                // possible cases:
                // 1. this: insert, other: retain
                // 2. this: retain, other: retain
                // 3. this: retain, other: delete
                // 4. this: insert, other: delete

                let (mut this_op, mut other_op) = this_iter.next_pair(&mut other_iter);
                if other_op.is_retain() {
                    // 1. this: insert, other: retain
                    // 2. this: retain, other: retain
                    this_op.compose_meta(&other_op);
                    let merged = delta.push(this_op);
                    let concat_rest = !other_iter.has_next() && !merged;
                    if concat_rest {
                        let vec = this_iter.rest();
                        if vec.is_empty() {
                            break;
                        }

                        let rest = Delta { vec };
                        delta = delta.concat(rest);
                        break;
                    }
                } else if other_op.is_delete() && this_op.is_retain() {
                    // 3. this: retain, other: delete
                    other_op.compose_meta(&this_op);
                    // other deletes the retained text
                    delta.push(other_op);
                } else {
                    // 4. this: insert, other: delete
                    // nothing to do here, because insert and delete have the same length
                }
            }
        }

        // debug_log::debug_dbg!(&delta);
        delta.chop()
    }

    fn concat(mut self, mut other: Self) -> Self {
        if !other.vec.is_empty() {
            let other_first = other.vec.remove(0);
            self.push(other_first);
            self.vec.extend(other.vec);
        }
        self
    }

    pub fn chop(mut self) -> Self {
        let last_op = self.vec.last();
        if let Some(last_op) = last_op {
            if last_op.is_retain() && last_op.meta().is_empty() {
                self.vec.pop();
            }
        }
        self
    }
}

impl<Value, M> IntoIterator for Delta<Value, M> {
    type Item = DeltaItem<Value, M>;

    type IntoIter = std::vec::IntoIter<DeltaItem<Value, M>>;
    fn into_iter(self) -> Self::IntoIter {
        self.vec.into_iter()
    }
}

impl<Value: DeltaValue, M: Meta> Default for Delta<Value, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Debug> DeltaValue for Vec<T> {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        self.extend(other);
        Ok(())
    }

    fn take(&mut self, length: usize) -> Self {
        let mut new = self.split_off(length);
        std::mem::swap(self, &mut new);
        new
    }

    fn length(&self) -> usize {
        self.len()
    }
}

impl DeltaValue for String {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        self.push_str(&other);
        Ok(())
    }

    fn take(&mut self, length: usize) -> Self {
        let mut new = self.split_off(length);
        std::mem::swap(self, &mut new);
        new
    }
    fn length(&self) -> usize {
        self.len()
    }
}

#[cfg(test)]
mod test {
    use super::{Delta, DeltaItem, DeltaType, Meta};

    #[derive(Debug, PartialEq, Clone, Default)]
    struct TestMeta {
        bold: Option<Option<bool>>,
        color: Option<Option<()>>,
    }

    impl Meta for TestMeta {
        fn empty() -> Self {
            TestMeta {
                bold: None,
                color: None,
            }
        }
        fn is_empty(&self) -> bool {
            self.bold.is_none() && self.color.is_none()
        }

        fn compose(
            &mut self,
            other: &Self,
            (this_type, other_type): (super::DeltaType, super::DeltaType),
        ) {
            if this_type != DeltaType::Delete && other_type != DeltaType::Delete {
                if let Some(other_bold) = other.bold {
                    self.bold = Some(other_bold);
                }
                if let Some(other_color) = other.color {
                    self.color = Some(other_color);
                }
            }
        }

        fn is_mergeable(&self, other: &Self) -> bool {
            self == other
        }

        fn merge(&mut self, _other: &Self) {}
    }

    type TestDelta = Delta<String, TestMeta>;
    const BOLD_META: TestMeta = TestMeta {
        bold: Some(Some(true)),
        color: None,
    };
    const COLOR_META: TestMeta = TestMeta {
        bold: None,
        color: Some(Some(())),
    };
    const EMPTY_META: TestMeta = TestMeta {
        bold: Some(None),
        color: Some(None),
    };
    const BOTH_META: TestMeta = TestMeta {
        bold: Some(Some(true)),
        color: Some(Some(())),
    };

    #[test]
    fn delta_push() {
        let mut a: Delta<String, ()> = Delta::new().insert("a".to_string());
        a.push(DeltaItem::Insert {
            insert: "b".to_string(),
            attributes: (),
        });
        assert_eq!(a, Delta::new().insert("ab".to_string()));
    }

    #[test]
    fn delta_compose() {
        let a: Delta<String, ()> = Delta::new().retain(3).insert("abcde".to_string());
        let b = Delta::new().retain(5).delete(6);
        assert_eq!(
            a.compose(b),
            Delta::new().retain(3).insert("ab".to_string()).delete(3)
        );

        let a: Delta<String, ()> = Delta::new().insert("123".to_string());
        let b = Delta::new().retain(1).insert("123".to_string());
        assert_eq!(a.compose(b), Delta::new().insert("112323".to_string()));
    }

    #[test]
    fn delete_failed() {
        let a: Delta<String, ()> = Delta::new()
            .retain(2)
            .insert("[31354]")
            .retain(1)
            .insert("[31354]")
            .retain(12)
            .insert("[31354]");
        let b: Delta<String, ()> = Delta::new().retain(27).delete(3);
        assert_eq!(
            a.compose(b),
            Delta::new()
                .retain(2)
                .insert("[31354]")
                .retain(1)
                .insert("[31354]")
                .retain(10)
                .insert("31354]")
                .delete(2)
        );
    }

    #[test]
    fn insert_insert() {
        let a = TestDelta::new().insert("a");
        let b = TestDelta::new().insert("b");
        assert_eq!(a.compose(b), TestDelta::new().insert("b").insert("a"));
    }

    #[test]
    fn insert_retain() {
        let a = TestDelta::new().insert("a");
        let b = TestDelta::new().retain_with_meta(1, BOLD_META);
        assert_eq!(
            a.compose(b),
            TestDelta::new().insert_with_meta("a", BOLD_META)
        );
    }

    #[test]
    fn insert_delete() {
        let a = TestDelta::new().insert("a");
        let b = TestDelta::new().delete(1);
        assert_eq!(a.compose(b), TestDelta::new());
    }

    #[test]
    fn delete_insert() {
        let a = TestDelta::new().delete(1);
        let b = TestDelta::new().insert("b");
        assert_eq!(a.compose(b), TestDelta::new().insert("b").delete(1));
    }

    #[test]
    fn delete_retain() {
        let a = TestDelta::new().delete(1);
        let b = TestDelta::new().retain_with_meta(1, BOLD_META);
        assert_eq!(
            a.compose(b),
            TestDelta::new().delete(1).retain_with_meta(1, BOLD_META)
        );
    }

    #[test]
    fn delete_delete() {
        let a = TestDelta::new().delete(1);
        let b = TestDelta::new().delete(1);
        assert_eq!(a.compose(b), TestDelta::new().delete(2));
    }

    #[test]
    fn retain_insert() {
        let a = TestDelta::new().retain_with_meta(1, BOLD_META);
        let b = TestDelta::new().insert("b");
        assert_eq!(
            a.compose(b),
            TestDelta::new().insert("b").retain_with_meta(1, BOLD_META)
        );
    }

    #[test]
    fn retain_retain() {
        let a = TestDelta::new().retain_with_meta(1, BOLD_META);
        let b = TestDelta::new().retain_with_meta(1, COLOR_META);
        assert_eq!(
            a.compose(b),
            TestDelta::new().retain_with_meta(1, BOTH_META)
        );
    }

    #[test]
    fn retain_delete() {
        let a = TestDelta::new().retain_with_meta(1, BOLD_META);
        let b = TestDelta::new().delete(1);
        assert_eq!(a.compose(b), TestDelta::new().delete(1));
    }

    #[test]
    fn delete_entire() {
        let a = TestDelta::new().retain(4).insert("abcde");
        let b = TestDelta::new().delete(9);
        assert_eq!(a.compose(b), TestDelta::new().delete(4));
    }

    #[test]
    fn retain_more() {
        let a = TestDelta::new().insert("abcde");
        let b = TestDelta::new().retain(10);
        assert_eq!(a.compose(b), TestDelta::new().insert("abcde"));
    }

    #[test]
    fn remove_meta() {
        let a = TestDelta::new().insert_with_meta("a", BOLD_META);
        let b = TestDelta::new().retain_with_meta(1, EMPTY_META);
        assert_eq!(
            a.compose(b),
            TestDelta::new().insert_with_meta("a", EMPTY_META)
        );
    }

    #[test]
    fn retain_start_opt() {
        let a = TestDelta::new()
            .insert_with_meta("a", BOLD_META)
            .insert("b")
            .insert_with_meta("c", BOLD_META)
            .delete(1);
        let b = TestDelta::new().retain(3).insert("d");
        let expect = TestDelta::new()
            .insert_with_meta("a", BOLD_META)
            .insert("b")
            .insert_with_meta("c", BOLD_META)
            .insert("d")
            .delete(1);
        assert_eq!(a.compose(b), expect);
    }

    #[test]
    fn retain_start_opt_split() {
        let a = TestDelta::new()
            .insert_with_meta("a", BOLD_META)
            .insert("b")
            .insert_with_meta("c", BOLD_META)
            .retain(5)
            .delete(1);
        let b = TestDelta::new().retain(4).insert("d");
        let expect = TestDelta::new()
            .insert_with_meta("a", BOLD_META)
            .insert("b")
            .insert_with_meta("c", BOLD_META)
            .retain(1)
            .insert("d")
            .retain(4)
            .delete(1);
        assert_eq!(a.compose(b), expect);
    }

    #[test]
    fn retain_end_opt() {
        let a = TestDelta::new()
            .insert_with_meta("a", BOLD_META)
            .insert("b")
            .insert_with_meta("c", BOLD_META);
        let b = TestDelta::new().delete(1);
        let expect = TestDelta::new()
            .insert("b")
            .insert_with_meta("c", BOLD_META);
        assert_eq!(a.compose(b), expect);
    }

    #[test]
    fn retain_end_opt_join() {
        let a = TestDelta::new()
            .insert_with_meta("a", BOLD_META)
            .insert("b")
            .insert_with_meta("c", BOLD_META)
            .insert("d")
            .insert_with_meta("e", BOLD_META)
            .insert("f");
        let b = TestDelta::new().retain(1).delete(1);
        let expect = TestDelta::new()
            .insert_with_meta("ac", BOLD_META)
            .insert("d")
            .insert_with_meta("e", BOLD_META)
            .insert("f");
        assert_eq!(a.compose(b), expect);
    }
}
