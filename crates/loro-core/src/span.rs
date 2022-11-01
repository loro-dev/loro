use std::fmt::Debug;

use crate::{
    change::Lamport,
    id::{ClientID, Counter, ID},
};
use rle::{HasLength, Mergable, Slice, Sliceable};

/// This struct supports reverse repr: `from` can be less than `to`.
/// We need this because it'll make merging deletions easier.
///
/// But we should use it behavior conservatively.
/// If it is not necessary to be reverse, it should not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CounterSpan {
    // TODO: should be private. user should not be able to change start from smaller than end to be greater than end
    pub start: Counter,
    // TODO: should be private
    pub end: Counter,
}

impl Debug for CounterSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{}~{}", self.start, self.end).as_str())
    }
}

impl CounterSpan {
    #[inline]
    pub fn new(from: Counter, to: Counter) -> Self {
        CounterSpan {
            start: from,
            end: to,
        }
    }

    #[inline]
    pub fn reverse(&mut self) {
        if self.start == self.end {
            return;
        }

        if self.start < self.end {
            (self.start, self.end) = (self.end - 1, self.start - 1);
        } else {
            (self.start, self.end) = (self.end + 1, self.start + 1);
        }
    }

    /// Make end greater than start
    pub fn normalize_(&mut self) {
        if self.end < self.start {
            self.reverse();
        }
    }

    #[inline(always)]
    pub fn direction(&self) -> i32 {
        if self.start < self.end {
            1
        } else {
            -1
        }
    }

    #[inline(always)]
    pub fn is_reversed(&self) -> bool {
        self.end < self.start
    }

    #[inline]
    pub fn min(&self) -> Counter {
        if self.start < self.end {
            self.start
        } else {
            self.end + 1
        }
    }

    #[inline(always)]
    pub fn max(&self) -> Counter {
        if self.start > self.end {
            self.start
        } else {
            self.end - 1
        }
    }

    #[inline(always)]
    pub fn end(&self) -> i32 {
        if self.start < self.end {
            self.end
        } else {
            self.start + 1
        }
    }

    #[inline]
    pub fn contains(&self, v: Counter) -> bool {
        if self.start < self.end {
            self.start <= v && v < self.end
        } else {
            self.start >= v && v > self.end
        }
    }

    pub fn set_start(&mut self, new_start: Counter) {
        if self.start < self.end {
            self.start = new_start.min(self.end);
        } else {
            self.start = new_start.max(self.end);
        }
    }

    pub fn set_end(&mut self, new_end: Counter) {
        if self.start < self.end {
            self.end = new_end.max(self.start);
        } else {
            self.end = new_end.min(self.start);
        }
    }
}

impl HasLength for CounterSpan {
    #[inline]
    fn content_len(&self) -> usize {
        if self.start < self.end {
            (self.end - self.start) as usize
        } else {
            (self.start - self.end) as usize
        }
    }
}

impl Sliceable for CounterSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from <= to);
        let len = to - from;
        assert!(len <= self.content_len());
        if self.start < self.end {
            CounterSpan {
                start: self.start + from as Counter,
                end: self.start + to as Counter,
            }
        } else {
            CounterSpan {
                start: self.start - from as Counter,
                end: self.start - to as Counter,
            }
        }
    }
}

impl Mergable for CounterSpan {
    #[inline]
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        // TODO: can use the similar logic as [DeleteSpan] to merge
        self.end == other.start && self.direction() == other.direction()
    }

    #[inline]
    fn merge(&mut self, other: &Self, _: &()) {
        self.end = other.end;
    }
}

/// This struct supports reverse repr: [CounterSpan]'s from can be less than to. But we should use it conservatively.
/// We need this because it'll make merging deletions easier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdSpan {
    pub client_id: ClientID,
    pub counter: CounterSpan,
}

impl IdSpan {
    #[inline]
    pub fn new(client_id: ClientID, from: Counter, to: Counter) -> Self {
        Self {
            client_id,
            counter: CounterSpan {
                start: from,
                end: to,
            },
        }
    }

    #[inline(always)]
    pub fn is_reversed(&self) -> bool {
        self.counter.end < self.counter.start
    }

    #[inline(always)]
    pub fn id_at_begin(&self) -> ID {
        ID::new(self.client_id, self.counter.start)
    }

    #[inline(always)]
    pub fn reverse(&mut self) {
        self.counter.reverse();
    }

    #[inline(always)]
    pub fn normalize_(&mut self) {
        self.counter.normalize_();
    }

    #[inline]
    pub fn min_id(&self) -> ID {
        ID::new(self.client_id, self.counter.min())
    }

    #[inline]
    pub fn end_id(&self) -> ID {
        ID::new(self.client_id, self.counter.end())
    }
}

impl HasLength for IdSpan {
    #[inline]
    fn content_len(&self) -> usize {
        self.counter.content_len()
    }
}

impl Sliceable for IdSpan {
    #[inline]
    fn slice(&self, from: usize, to: usize) -> Self {
        IdSpan {
            client_id: self.client_id,
            counter: self.counter.slice(from, to),
        }
    }
}

impl Mergable for IdSpan {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        self.client_id == other.client_id && self.counter.is_mergable(&other.counter, &())
    }

    fn merge(&mut self, other: &Self, _: &()) {
        self.counter.merge(&other.counter, &())
    }
}

pub trait HasId {
    fn id_start(&self) -> ID;
}

pub trait HasCounter {
    fn ctr_start(&self) -> Counter;
}

pub trait HasCounterSpan: HasCounter + HasLength {
    fn ctr_end(&self) -> Counter {
        self.ctr_start() + self.atom_len() as Counter
    }

    fn ctr_last(&self) -> Counter {
        self.ctr_start() + self.atom_len() as Counter - 1
    }

    fn ctr_span(&self) -> CounterSpan {
        CounterSpan {
            start: self.ctr_start(),
            end: self.ctr_end(),
        }
    }
}

impl<T: HasCounter + HasLength> HasCounterSpan for T {}

impl<T: HasId> HasCounter for T {
    #[inline]
    fn ctr_start(&self) -> Counter {
        self.id_start().counter
    }
}

pub trait HasIdSpan: HasId + HasLength {
    fn intersect<T: HasIdSpan>(&self, other: &T) -> bool {
        let self_start = self.id_start();
        let other_start = self.id_start();
        if self_start.client_id != other_start.client_id {
            false
        } else {
            let self_start = self_start.counter;
            let other_start = other_start.counter;
            let self_end = self.id_end().counter;
            let other_end = other.id_end().counter;
            self_start < other_end && other_start < self_end
        }
    }

    fn id_span(&self) -> IdSpan {
        let id = self.id_start();
        IdSpan::new(
            id.client_id,
            id.counter,
            id.counter + self.content_len() as Counter,
        )
    }

    fn id_end(&self) -> ID {
        self.id_start().inc(self.content_len() as i32)
    }

    fn id_last(&self) -> ID {
        self.id_start().inc(self.content_len() as i32 - 1)
    }

    fn contains_id(&self, id: ID) -> bool {
        let id_start = self.id_start();
        if id.client_id != id_start.client_id {
            return false;
        }

        id_start.counter <= id.counter
            && id.counter < id_start.counter + self.content_len() as Counter
    }
}
impl<T: HasId + HasLength> HasIdSpan for T {}

pub trait HasLamport {
    fn lamport(&self) -> Lamport;
}

pub trait HasLamportSpan: HasLamport + HasLength {
    fn lamport_end(&self) -> Lamport {
        self.lamport() + self.content_len() as Lamport
    }

    fn lamport_last(&self) -> Lamport {
        self.lamport() + self.content_len() as Lamport - 1
    }
}
impl<T: HasLamport + HasLength> HasLamportSpan for T {}

impl HasId for IdSpan {
    #[inline]
    fn id_start(&self) -> ID {
        self.min_id()
    }
}

#[cfg(test)]
mod test_id_span {
    use rle::RleVecWithIndex;

    use super::*;

    macro_rules! id_spans {
        ($([$client_id:expr, $from:expr, $to:expr]),*) => {
            {
                let mut id_spans = RleVecWithIndex::new();
                $(
                    id_spans.push(IdSpan {
                        client_id: $client_id,
                        counter: CounterSpan::new($from, $to),
                    });
                )*
                id_spans
            }
        };
    }

    #[test]
    fn test_id_span_rle_vec() {
        let mut id_span_vec = RleVecWithIndex::new();
        id_span_vec.push(IdSpan {
            client_id: 0,
            counter: CounterSpan::new(0, 2),
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.atom_len(), 2);
        id_span_vec.push(IdSpan {
            client_id: 0,
            counter: CounterSpan::new(2, 4),
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.atom_len(), 4);
        id_span_vec.push(IdSpan {
            client_id: 2,
            counter: CounterSpan::new(2, 4),
        });
        assert_eq!(id_span_vec.merged_len(), 2);
        assert_eq!(id_span_vec.atom_len(), 6);
    }

    #[test]
    fn slice() {
        let id_span_vec = id_spans!([0, 0, 2], [0, 2, 4], [2, 2, 4]);
        let slice: Vec<IdSpan> = id_span_vec.slice_iter(2, 5).map(|x| x.into()).collect();
        assert_eq!(slice, id_spans!([0, 2, 4], [2, 2, 3]).to_vec());
    }

    #[test]
    fn backward() {
        let id_span_vec = id_spans!([0, 100, 98], [0, 98, 90], [2, 2, 4], [2, 8, 4]);
        let slice: Vec<IdSpan> = id_span_vec.slice_iter(5, 14).map(|x| x.into()).collect();
        assert_eq!(slice, id_spans!([0, 95, 90], [2, 2, 4], [2, 8, 6]).to_vec());
    }
}

impl<'a> From<Slice<'a, IdSpan>> for IdSpan {
    fn from(slice: Slice<'a, IdSpan>) -> Self {
        slice.value.slice(slice.start, slice.end)
    }
}
