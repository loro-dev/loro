use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
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

#[derive(Debug, EnumAsInner, PartialEq, Eq, Clone, Serialize)]
pub enum DeltaItem<Value, Meta> {
    Retain { len: usize, meta: Option<Meta> },
    Insert { value: Value, meta: Option<Meta> },
    Delete(usize),
}

pub trait Meta: Debug + Clone + PartialEq {
    fn empty() -> Self;
    fn is_empty(&self) -> bool;
}

impl Meta for () {
    fn empty() -> Self {}
    fn is_empty(&self) -> bool {
        true
    }
}

pub trait DeltaValue: Debug + HasLength + Sliceable + Clone + PartialEq {
    fn value_extend(&mut self, other: Self);
    fn take(&mut self, length: usize) -> Self;
}

impl<Value: DeltaValue, M: Meta> DeltaItem<Value, M> {
    pub fn meta(&self) -> &Option<M> {
        match self {
            DeltaItem::Insert { meta, .. } => meta,
            DeltaItem::Retain { meta, .. } => meta,
            _ => &None,
        }
    }

    pub fn set_meta(&mut self, meta: Option<M>) {
        match self {
            DeltaItem::Insert { meta: m, .. } => *m = meta,
            DeltaItem::Retain { meta: m, .. } => *m = meta,
            _ => {}
        }
    }

    fn take_meta(&mut self) -> Option<M> {
        match self {
            DeltaItem::Insert { meta, .. } => std::mem::take(meta),
            DeltaItem::Retain { meta, .. } => std::mem::take(meta),
            _ => None,
        }
    }

    // change self-length to self.len()-length
    // and return the taken one.
    pub(crate) fn take(&mut self, length: usize) -> Self {
        match self {
            DeltaItem::Insert { value, meta } => {
                let v = value.take(length);
                Self::Insert {
                    value: v,
                    meta: meta.clone(),
                }
            }
            DeltaItem::Retain { len, meta } => {
                *len -= length;
                Self::Retain {
                    len: length,
                    meta: meta.clone(),
                }
            }
            DeltaItem::Delete(len) => {
                *len -= length;
                Self::Delete(length)
            }
        }
    }

    pub fn is_retain(&self) -> bool {
        matches!(self, Self::Retain { .. })
    }

    pub fn is_insert(&self) -> bool {
        matches!(self, Self::Insert { .. })
    }

    pub fn is_delete(&self) -> bool {
        matches!(self, Self::Delete(_))
    }
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

pub struct DeltaIterator<V, M: Meta> {
    // The reversed Vec uses pop() to simulate getting the first element each time
    ops: Vec<DeltaItem<V, M>>,
    // index: usize,
    // offset: usize,
}

impl<V: DeltaValue, M: Meta> DeltaIterator<V, M> {
    fn new(ops: Vec<DeltaItem<V, M>>) -> Self {
        Self {
            ops,
            // index: 0,
            // offset: 0,
        }
    }

    fn next<L: Into<Option<usize>>>(&mut self, len: L) -> DeltaItem<V, M> {
        self.next_impl(len.into())
    }

    fn next_impl(&mut self, mut len: Option<usize>) -> DeltaItem<V, M> {
        if len.is_none() {
            len = Some(usize::MAX)
        }
        let length = len.unwrap();

        let next_op = self.peek_mut();
        if next_op.is_none() {
            return DeltaItem::Retain {
                len: usize::MAX,
                meta: None,
            };
        }
        let op = next_op.unwrap();
        let op_length = op.content_len();
        if length < op_length {
            // a part of the peek op
            op.take(length)
        } else {
            self.take_peek().unwrap()
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
            op.content_len()
        } else {
            usize::MAX
        }
    }

    // fn peek_is_retain(&self) -> bool {
    //     if let Some(op) = self.peek() {
    //         op.is_retain()
    //     } else {
    //         // default
    //         true
    //     }
    // }

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
        self.push(DeltaItem::Retain {
            len,
            meta: Some(meta),
        });
        self
    }

    pub fn insert_with_meta<V: Into<Value>>(mut self, value: V, meta: M) -> Self {
        self.push(DeltaItem::Insert {
            value: value.into(),
            meta: Some(meta),
        });
        self
    }

    pub fn delete(mut self, len: usize) -> Self {
        if len == 0 {
            return self;
        }
        self.push(DeltaItem::Delete(len));
        self
    }
    pub fn retain(mut self, len: usize) -> Self {
        if len == 0 {
            return self;
        }

        self.push(DeltaItem::Retain { len, meta: None });
        self
    }

    pub fn insert<V: Into<Value>>(mut self, value: V) -> Self {
        self.push(DeltaItem::Insert {
            value: value.into(),
            meta: None,
        });
        self
    }

    // If the new_op is merged, return true
    pub fn push(&mut self, new_op: DeltaItem<Value, M>) -> bool {
        let mut index = self.vec.len();
        let last_op = self.vec.pop();
        if let Some(mut last_op) = last_op {
            if new_op.is_delete() && last_op.is_delete() {
                *last_op.as_delete_mut().unwrap() += new_op.content_len();
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
            if new_op.meta() == last_op.meta() {
                if new_op.is_insert() && last_op.is_insert() {
                    let value = last_op.as_insert_mut().unwrap().0;
                    value.value_extend(new_op.into_insert().unwrap().0);
                    self.vec.push(last_op);
                    return true;
                } else if new_op.is_retain() && last_op.is_retain() {
                    *last_op.as_retain_mut().unwrap().0 += new_op.content_len();
                    self.vec.push(last_op);
                    return true;
                }
            }

            self.vec.push(last_op);
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
    pub fn compose(self, other: Self) -> Self {
        let mut this_iter = self.into_op_iter();
        let mut other_iter = other.into_op_iter();
        let mut ops = vec![];
        let first_other = other_iter.peek();
        if let Some(first_other) = first_other {
            if first_other.is_retain() && first_other.meta().is_none() {
                let mut first_left = first_other.content_len();
                let mut first_this = this_iter.peek();
                while let Some(first_this_inner) = first_this {
                    if first_this_inner.is_insert() && first_this_inner.content_len() <= first_left
                    {
                        first_left -= first_this_inner.content_len();
                        ops.push(this_iter.next(None));
                        first_this = this_iter.peek();
                    } else {
                        break;
                    }
                }
                if first_other.content_len() - first_left > 0 {
                    other_iter.next(first_other.content_len() - first_left);
                }
            }
        }
        let mut delta = Delta { vec: ops };
        while this_iter.has_next() || other_iter.has_next() {
            if other_iter.peek_is_insert() {
                delta.push(other_iter.next(None));
            } else if this_iter.peek_is_delete() {
                delta.push(this_iter.next(None));
            } else {
                let length = this_iter.peek_length().min(other_iter.peek_length());
                let mut this_op = this_iter.next(length);
                let mut other_op = other_iter.next(length);
                if other_op.is_retain() {
                    let meta = if other_op.meta().is_none() {
                        this_op.take_meta()
                    } else if other_op.meta().as_ref().unwrap().is_empty() {
                        None
                    } else {
                        other_op.take_meta()
                    };
                    let new_op = if this_op.is_retain() {
                        DeltaItem::Retain { len: length, meta }
                    } else {
                        this_op.set_meta(meta);
                        this_op
                    };
                    let merged = delta.push(new_op);
                    let concat_rest = !other_iter.has_next() && !merged;
                    if concat_rest {
                        let vec = this_iter.rest();
                        if vec.is_empty() {
                            return delta.chop();
                        }
                        let rest = Delta { vec };
                        return delta.concat(rest).chop();
                    }
                } else if other_op.is_delete() && this_op.is_retain() {
                    delta.push(other_op);
                }
            }
        }
        delta.chop()
    }

    fn concat(mut self, mut other: Self) -> Self {
        if !other.vec.is_empty() {
            let other_first = other.vec.remove(0);
            self.push(other_first);
            self.vec.value_extend(other.vec);
        }
        self
    }

    fn chop(mut self) -> Self {
        let last_op = self.vec.last();
        if let Some(last_op) = last_op {
            if last_op.is_retain() && last_op.meta().is_none() {
                self.vec.pop();
            }
        }
        self
    }
}

impl<Value: DeltaValue, M: Meta> Default for Delta<Value, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + Debug> DeltaValue for Vec<T> {
    fn value_extend(&mut self, other: Self) {
        self.extend(other)
    }

    fn take(&mut self, length: usize) -> Self {
        let mut new = self.split_off(length);
        std::mem::swap(self, &mut new);
        new
    }
}

impl DeltaValue for String {
    fn value_extend(&mut self, other: Self) {
        self.push_str(&other)
    }

    fn take(&mut self, length: usize) -> Self {
        let mut new = self.split_off(length);
        std::mem::swap(self, &mut new);
        new
    }
}

#[cfg(test)]
mod test {
    use super::{Delta, DeltaItem, Meta};

    #[derive(Debug, PartialEq, Clone, Default)]
    struct TestMeta {
        bold: Option<bool>,
        color: Option<()>,
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
    }

    type TestDelta = Delta<String, TestMeta>;
    const BOLD_META: TestMeta = TestMeta {
        bold: Some(true),
        color: None,
    };
    const COLOR_META: TestMeta = TestMeta {
        bold: None,
        color: Some(()),
    };
    const EMPTY_META: TestMeta = TestMeta {
        bold: None,
        color: None,
    };

    #[test]
    fn delta_push() {
        let mut a: Delta<String, ()> = Delta::new().insert("a".to_string());
        a.push(DeltaItem::Insert {
            value: "b".to_string(),
            meta: None,
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
            TestDelta::new().retain_with_meta(1, COLOR_META)
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
        assert_eq!(a.compose(b), TestDelta::new().insert("a"));
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
