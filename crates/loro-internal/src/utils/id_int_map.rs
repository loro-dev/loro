use itertools::Either;
use loro_common::{HasCounter, HasCounterSpan, HasId, HasIdSpan, IdSpan, ID};
use rle::HasLength;
use std::collections::BTreeMap;

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
        if cfg!(debug_assertions) {
            let target = self.get(id_span.id_start());
            assert!(
                target.is_none(),
                "ID already exists {id_span:?} {target:?} {:#?}",
                self
            );
        }

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

                let pos = match vec.binary_search_by(|x| x.0.id_start().cmp(&id_span.id_start())) {
                    Ok(_) => unreachable!("ID already exists"),
                    Err(i) => i,
                };

                if pos > 0 {
                    if let Some(last) = vec.get_mut(pos - 1) {
                        if last.0.id_end() == id_span.id_start()
                            && last.1 + last.0.atom_len() as i32 == value
                        {
                            // can merge
                            last.0.counter.end += len;
                            return;
                        }
                    }
                }

                vec.insert(pos, (id_span, value));
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
        let ans = match &self.inner {
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
        };
        ans
    }

    /// Call `next` for each key-value pair that is in the given span.
    /// It's guaranteed that the keys are in ascending order.
    pub fn get_values_in_span(&self, target: IdSpan, mut next: impl FnMut(IdSpan, i32)) {
        let target_peer = target.client_id;
        match &self.inner {
            Either::Left(map) => {
                let last = map
                    .range(..&target.id_start())
                    .next_back()
                    .and_then(|(id, v)| {
                        if id.peer != target_peer {
                            None
                        } else if id.counter + v.len > target.ctr_start() {
                            Some((id, v))
                        } else {
                            None
                        }
                    });

                let iter = map.range(&target.id_start()..);
                for (entry_key, value) in last.into_iter().chain(iter) {
                    if entry_key.peer > target_peer {
                        break;
                    }

                    if entry_key.counter >= target.ctr_end() {
                        break;
                    }

                    assert_eq!(entry_key.peer, target_peer);
                    let cur_span = &IdSpan::new(
                        target_peer,
                        entry_key.counter,
                        entry_key.counter + value.len,
                    );

                    let next_span = cur_span.get_intersection(&target).unwrap();
                    (next)(
                        next_span,
                        value.value + next_span.counter.start - entry_key.counter,
                    );
                }
            }
            Either::Right(vec) => {
                for (id_span, value) in vec.iter() {
                    if id_span.client_id < target_peer {
                        continue;
                    }

                    if id_span.client_id > target_peer {
                        break;
                    }

                    if target.ctr_start() >= id_span.ctr_end() {
                        continue;
                    }

                    if target.ctr_end() <= id_span.counter.start {
                        break;
                    }

                    assert_eq!(id_span.client_id, target_peer);
                    let next_span = id_span.get_intersection(&target).unwrap();
                    (next)(
                        next_span,
                        *value + next_span.counter.start - id_span.counter.start,
                    );
                }
            }
        }
    }

    /// If the given item has overlapped section with the content in the map,
    /// split the item into pieces where each piece maps to a continuous series of values or maps to none.
    pub(crate) fn split<'a, T: HasIdSpan + generic_btree::rle::Sliceable + Clone + 'a>(
        &'a self,
        item: T,
    ) -> impl Iterator<Item = (T, Option<i32>)> + 'a {
        let len = item.rle_len();
        let span = item.id_span();
        // PERF: we may avoid this alloc if get_values_in_span returns an iter
        let mut ans = Vec::new();
        let mut ctr_start = span.ctr_start();
        let mut index: i32 = 0;
        self.get_values_in_span(span, |id_span: IdSpan, v| {
            if id_span.counter.start > ctr_start {
                // There is a gap between the previous value and the current value
                ans.push((
                    item.slice(
                        index as usize..(index + id_span.counter.start - ctr_start) as usize,
                    ),
                    None,
                ));
                index += id_span.counter.start - ctr_start;
            }

            ans.push((
                item.slice(
                    index as usize..(index + id_span.counter.end - id_span.counter.start) as usize,
                ),
                Some(v),
            ));
            index += id_span.counter.end - id_span.counter.start;
            ctr_start = id_span.ctr_end();
        });

        if ans.is_empty() && len > 0 {
            ans.push((item, None));
        } else if index as usize != len {
            ans.push((item.slice(index as usize..len), None));
        }

        ans.into_iter()
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

        let mut called = 0;
        map.get_values_in_span(IdSpan::new(0, 3, 66), |id_span, value| {
            called += 1;
            assert_eq!(id_span, IdSpan::new(0, 3, 66));
            assert_eq!(value, 3);
        });
        assert_eq!(called, 1);

        let mut called = Vec::new();
        map.get_values_in_span(IdSpan::new(3, 0, 10), |id_span, value| {
            called.push((id_span, value));
        });
        assert_eq!(
            called,
            vec![
                (IdSpan::new(3, 0, 1), 400),
                (IdSpan::new(3, 2, 3), 401),
                (IdSpan::new(3, 4, 5), 402),
                (IdSpan::new(3, 6, 7), 403),
                (IdSpan::new(3, 8, 9), 404),
            ]
        );
    }

    #[test]
    fn test_get_values() {
        let mut map = IdIntMap::new();
        map.insert(IdSpan::new(0, 3, 5));
        map.insert(IdSpan::new(0, 0, 1));
        map.insert(IdSpan::new(0, 2, 3));

        let mut called = Vec::new();
        map.get_values_in_span(IdSpan::new(0, 0, 10), |id_span, value| {
            called.push((id_span, value));
        });
        assert_eq!(
            called,
            vec![
                (IdSpan::new(0, 0, 1), 2),
                (IdSpan::new(0, 2, 3), 3),
                (IdSpan::new(0, 3, 5), 0),
            ]
        );
    }
}
