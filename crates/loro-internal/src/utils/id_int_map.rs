use std::collections::BTreeMap;

use itertools::Either;
use loro_common::{HasCounter, HasId, IdSpan, ID};
use rle::HasLength;

/// A map that maps spans of continuous [ID]s to spans of continuous integers.
///
/// It can merge spans that are adjacent to each other.
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
            inner: Either::Left(Default::default()),
            next_value: 0,
        }
    }

    pub fn insert(&mut self, id_span: IdSpan) {
        let value = self.next_value;
        let len = id_span.atom_len() as i32;
        self.next_value += len;
        match &mut self.inner {
            Either::Left(map) => {
                map.insert(id_span.id_start(), Value { len, value });
            }
            Either::Right(vec) => {
                if vec.len() == MAX_VEC_LEN {
                    let mut map = BTreeMap::new();
                    for (id_span, value) in vec.drain(..) {
                        map.insert(id_span.id_start(), Value { len, value });
                    }
                    self.inner = Either::Left(map);
                    self.insert(id_span);
                } else {
                    vec.push((id_span, value));
                }
            }
        }
    }

    pub fn get(&self, target: ID) -> Option<i32> {
        match &self.inner {
            Either::Left(map) => map.range(..=&target).last().and_then(|(entry_key, value)| {
                if entry_key.peer != target.peer {
                    None
                } else if entry_key.counter + value.len > target.counter {
                    Some(value.value + target.counter - entry_key.counter)
                } else {
                    None
                }
            }),
            Either::Right(vec) => vec
                .iter()
                .rev()
                .find(|(id_span, _)| id_span.contains(target))
                .map(|(id_span, value)| *value + target.counter - id_span.ctr_start()),
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
        assert_eq!(map.get(ID::new(0, 10)).unwrap(), 10);
        assert_eq!(map.get(ID::new(1, 10)).unwrap(), 110);
        assert_eq!(map.get(ID::new(2, 10)).unwrap(), 210);
        assert_eq!(map.get(ID::new(0, 0)).unwrap(), 0);
        assert_eq!(map.get(ID::new(1, 0)).unwrap(), 100);
        assert_eq!(map.get(ID::new(2, 0)).unwrap(), 200);
        assert_eq!(map.get(ID::new(999, 99)).unwrap(), 399);

        for i in 0..100 {
            map.insert(IdSpan::new(3, i, i + 1));
        }

        assert_eq!(map.get(ID::new(0, 10)).unwrap(), 10);
        assert_eq!(map.get(ID::new(1, 10)).unwrap(), 110);
        assert_eq!(map.get(ID::new(2, 10)).unwrap(), 210);
        assert_eq!(map.get(ID::new(0, 0)).unwrap(), 0);
        assert_eq!(map.get(ID::new(1, 0)).unwrap(), 100);
        assert_eq!(map.get(ID::new(2, 0)).unwrap(), 200);
        assert_eq!(map.get(ID::new(999, 99)).unwrap(), 399);

        for i in 0..100 {
            assert_eq!(map.get(ID::new(3, i)).unwrap(), i + 400);
        }
    }
}
