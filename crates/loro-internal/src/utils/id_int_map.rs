use std::collections::BTreeMap;

use itertools::Either;
use loro_common::{HasCounter, HasCounterSpan, HasId, HasIdSpan, IdSpan, ID};
use rle::HasLength;

/// A map that maps spans of continuous [ID]s to spans of continuous integers.
///
/// It can merge spans that are adjacent to each other.
/// The value is automatically incremented by the length of the inserted span.
#[derive(Debug)]
pub struct IdIntMap {
    inner: Either<BTreeMap<ID, Value>, Vec<(IdSpan, i32)>>,
    next_value: i32,
}

const MAX_VEC_LEN: usize = 16;

#[derive(Debug)]
struct Value {
    len: i32,
    value: i32,
}

impl IdIntMap {
    pub fn new() -> Self {
        Self {
            inner: Either::Right(Default::default()),
            next_value: 0,
        }
    }

    pub fn insert(&mut self, id_span: IdSpan) {
        match &mut self.inner {
            Either::Left(map) => {
                let value = self.next_value;
                let len = id_span.atom_len() as i32;
                self.next_value += len;

                let id = id_span.id_start();
                match map.range_mut(..&id).last() {
                    Some(last)
                        if last.0.peer == id.peer
                            && last.0.counter + last.1.len == id.counter
                            && last.1.value + last.1.len == value =>
                    {
                        // merge
                        last.1.len += len;
                    }
                    _ => {
                        map.insert(id, Value { len, value });
                    }
                }
            }
            Either::Right(vec) => {
                if vec.len() == MAX_VEC_LEN {
                    // convert to map and insert
                    self.escalate_to_map();
                    self.insert(id_span);
                    return;
                }

                let value = self.next_value;
                let len = id_span.atom_len() as i32;
                self.next_value += len;

                if let Some(last) = vec.last_mut() {
                    if last.0.id_end() == id_span.id_start()
                        && last.1 + last.0.atom_len() as i32 == value
                    {
                        // can merge
                        last.0.counter.end += len;
                        return;
                    }
                }

                vec.push((id_span, value));
            }
        }
    }

    fn escalate_to_map(&mut self) {
        let Either::Right(vec) = &mut self.inner else {
            return;
        };
        let mut map = BTreeMap::new();
        for (id_span, value) in vec.drain(..) {
            map.insert(
                id_span.id_start(),
                Value {
                    len: id_span.atom_len() as i32,
                    value,
                },
            );
        }

        self.inner = Either::Left(map);
    }

    /// Return (value, length) that starts at the given ID.
    pub fn get(&self, target: ID) -> Option<(i32, usize)> {
        match &self.inner {
            Either::Left(map) => map.range(..=&target).last().and_then(|(entry_key, value)| {
                if entry_key.peer != target.peer {
                    None
                } else if entry_key.counter + value.len > target.counter {
                    Some((
                        value.value + target.counter - entry_key.counter,
                        (entry_key.counter + value.len - target.counter) as usize,
                    ))
                } else {
                    None
                }
            }),
            Either::Right(vec) => vec
                .iter()
                .rev()
                .find(|(id_span, _)| id_span.contains(target))
                .map(|(id_span, value)| {
                    (
                        *value + target.counter - id_span.ctr_start(),
                        (id_span.ctr_end() - target.counter) as usize,
                    )
                }),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_basic() {
        let mut map = IdIntMap::new();
        map.insert(IdSpan::new(0, 0, 10));
        map.insert(IdSpan::new(0, 10, 100));
        map.insert(IdSpan::new(1, 0, 100));
        map.insert(IdSpan::new(2, 0, 100));
        map.insert(IdSpan::new(999, 0, 100));
        assert!(map.inner.is_right());
        assert_eq!(map.get(ID::new(0, 10)).unwrap().0, 10);
        assert_eq!(map.get(ID::new(1, 10)).unwrap().0, 110);
        assert_eq!(map.get(ID::new(2, 10)).unwrap().0, 210);
        assert_eq!(map.get(ID::new(0, 0)).unwrap().0, 0);
        assert_eq!(map.get(ID::new(1, 0)).unwrap().0, 100);
        assert_eq!(map.get(ID::new(2, 0)).unwrap().0, 200);
        assert_eq!(map.get(ID::new(999, 99)).unwrap().0, 399);

        for i in 0..100 {
            map.insert(IdSpan::new(3, i * 2, i * 2 + 1));
        }

        assert!(map.inner.is_left());
        assert_eq!(map.get(ID::new(0, 10)).unwrap().0, 10);
        assert_eq!(map.get(ID::new(1, 10)).unwrap().0, 110);
        assert_eq!(map.get(ID::new(2, 10)).unwrap().0, 210);
        assert_eq!(map.get(ID::new(0, 0)).unwrap().0, 0);
        assert_eq!(map.get(ID::new(1, 0)).unwrap().0, 100);
        assert_eq!(map.get(ID::new(2, 0)).unwrap().0, 200);
        assert_eq!(map.get(ID::new(999, 99)).unwrap().0, 399);
        for i in 0..100 {
            assert_eq!(map.get(ID::new(3, i * 2)).unwrap().0, i + 400, "i = {i}");
        }
    }
}
