use std::fmt::Debug;

use crate::{Counter, IdSpanVector, Lamport, PeerID, ID};
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

pub trait HasLamport {
    fn lamport(&self) -> Lamport;
}

pub trait HasLamportSpan: HasLamport + rle::HasLength {
    /// end is the exclusive end, last the inclusive end.
    fn lamport_end(&self) -> Lamport {
        self.lamport() + self.content_len() as Lamport
    }

    /// end is the exclusive end, last the inclusive end.
    fn lamport_last(&self) -> Lamport {
        self.lamport() + self.content_len() as Lamport - 1
    }
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
    pub fn bidirectional(&self) -> bool {
        (self.end - self.start).abs() == 1
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

    pub fn set_min(&mut self, min: Counter) {
        if self.start < self.end {
            self.start = min;
        } else {
            self.end = min - 1;
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
    /// Normalized end value.
    ///
    /// This is different from end. start may be greater than end. This is the max of start+1 and end
    pub fn norm_end(&self) -> i32 {
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

    /// if we can merge element on the left, this method return the last atom of it
    fn prev_pos(&self) -> i32 {
        if self.start < self.end {
            self.start - 1
        } else {
            self.start + 1
        }
    }

    /// if we can merge element on the right, this method return the first atom of it
    fn next_pos(&self) -> i32 {
        self.end
    }

    fn get_intersection(&self, counter: &CounterSpan) -> Option<Self> {
        let start = self.start.max(counter.start);
        let end = self.end.min(counter.end);
        if start < end {
            Some(CounterSpan { start, end })
        } else {
            None
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
        match (self.bidirectional(), other.bidirectional()) {
            (true, true) => self.start + 1 == other.start || self.start == other.start + 1,
            (true, false) => self.start == other.prev_pos(),
            (false, true) => self.next_pos() == other.start,
            (false, false) => {
                self.next_pos() == other.start && self.direction() == other.direction()
            }
        }
    }

    #[inline]
    fn merge(&mut self, other: &Self, _: &()) {
        match (self.bidirectional(), other.bidirectional()) {
            (true, true) => {
                if self.start + 1 == other.start {
                    self.end = self.start + 2;
                } else if self.start - 1 == other.start {
                    self.end = self.start - 2;
                }
            }
            (true, false) => self.end = other.end,
            (false, true) => self.end += self.direction(),
            (false, false) => {
                self.end = other.end;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LamportSpan {
    pub start: Lamport,
    pub end: Lamport,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdLpSpan {
    pub peer: PeerID,
    pub lamport: LamportSpan,
}

impl HasLength for IdLpSpan {
    fn content_len(&self) -> usize {
        (self.lamport.end - self.lamport.start) as usize
    }
}

impl IdLpSpan {
    pub fn new(peer: PeerID, from: Lamport, to: Lamport) -> Self {
        Self {
            peer,
            lamport: LamportSpan {
                start: from,
                end: to,
            },
        }
    }
}

/// This struct supports reverse repr: [CounterSpan]'s from can be less than to. But we should use it conservatively.
/// We need this because it'll make merging deletions easier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdSpan {
    pub peer: PeerID,
    pub counter: CounterSpan,
}

impl IdSpan {
    #[inline]
    pub fn new(peer: PeerID, from: Counter, to: Counter) -> Self {
        Self {
            peer,
            counter: CounterSpan {
                start: from,
                end: to,
            },
        }
    }

    #[inline]
    pub fn contains(&self, id: ID) -> bool {
        self.peer == id.peer && self.counter.contains(id.counter)
    }

    #[inline(always)]
    pub fn is_reversed(&self) -> bool {
        self.counter.end < self.counter.start
    }

    #[inline(always)]
    pub fn reverse(&mut self) {
        self.counter.reverse();
    }

    #[inline(always)]
    pub fn normalize_(&mut self) {
        self.counter.normalize_();
    }

    /// This is different from id_start. id_start may be greater than id_end, but this is the min of id_start and id_end-1
    #[inline]
    pub fn norm_id_start(&self) -> ID {
        ID::new(self.peer, self.counter.min())
    }

    /// This is different from id_end. id_start may be greater than id_end. This is the max of id_start+1 and id_end
    #[inline]
    pub fn norm_id_end(&self) -> ID {
        ID::new(self.peer, self.counter.norm_end())
    }

    pub fn to_id_span_vec(self) -> IdSpanVector {
        let mut out = IdSpanVector::default();
        out.insert(self.peer, self.counter);
        out
    }

    pub fn get_intersection(&self, other: &Self) -> Option<Self> {
        if self.peer != other.peer {
            return None;
        }

        let counter = self.counter.get_intersection(&other.counter)?;
        Some(Self {
            peer: self.peer,
            counter,
        })
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
            peer: self.peer,
            counter: self.counter.slice(from, to),
        }
    }
}

impl Mergable for IdSpan {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        self.peer == other.peer && self.counter.is_mergable(&other.counter, &())
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
    /// end is the exclusive end, last the inclusive end.
    fn ctr_end(&self) -> Counter {
        self.ctr_start() + self.atom_len() as Counter
    }

    /// end is the exclusive end, last the inclusive end.
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
        if self_start.peer != other_start.peer {
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
            id.peer,
            id.counter,
            id.counter + self.content_len() as Counter,
        )
    }

    /// end is the exclusive end, last the inclusive end.
    fn id_end(&self) -> ID {
        self.id_start().inc(self.content_len() as i32)
    }

    /// end is the exclusive end, last the inclusive end.
    fn id_last(&self) -> ID {
        self.id_start().inc(self.content_len() as i32 - 1)
    }

    fn contains_id(&self, id: ID) -> bool {
        let id_start = self.id_start();
        if id.peer != id_start.peer {
            return false;
        }

        id_start.counter <= id.counter
            && id.counter < id_start.counter + self.content_len() as Counter
    }
}
impl<T: HasId + HasLength> HasIdSpan for T {}

impl<T: HasLamport + HasLength> HasLamportSpan for T {}

impl HasId for IdSpan {
    #[inline]
    fn id_start(&self) -> ID {
        self.norm_id_start()
    }
}

impl<'a> From<Slice<'a, IdSpan>> for IdSpan {
    fn from(slice: Slice<'a, IdSpan>) -> Self {
        slice.value.slice(slice.start, slice.end)
    }
}

impl HasId for (&PeerID, &CounterSpan) {
    fn id_start(&self) -> ID {
        ID {
            peer: *self.0,
            counter: self.1.min(),
        }
    }
}

impl HasId for (PeerID, CounterSpan) {
    fn id_start(&self) -> ID {
        ID {
            peer: self.0,
            counter: self.1.min(),
        }
    }
}

impl From<ID> for IdSpan {
    fn from(value: ID) -> Self {
        Self::new(value.peer, value.counter, value.counter + 1)
    }
}

#[cfg(test)]
mod test_id_span {
    use rle::RleVecWithIndex;

    use super::*;

    macro_rules! id_spans {
        ($([$peer:expr, $from:expr, $to:expr]),*) => {
            {
                let mut id_spans = RleVecWithIndex::new();
                $(
                    id_spans.push(IdSpan {
                        peer: $peer,
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
            peer: 0,
            counter: CounterSpan::new(0, 2),
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.atom_len(), 2);
        id_span_vec.push(IdSpan {
            peer: 0,
            counter: CounterSpan::new(2, 4),
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.atom_len(), 4);
        id_span_vec.push(IdSpan {
            peer: 2,
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

    #[test]
    fn merge() {
        let mut a = CounterSpan::new(0, 2);
        let b = CounterSpan::new(2, 1);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, CounterSpan::new(0, 3));

        let mut a = CounterSpan::new(3, 2);
        let b = CounterSpan::new(2, 1);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, CounterSpan::new(3, 1));

        let mut a = CounterSpan::new(4, 2);
        let b = CounterSpan::new(2, 3);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, CounterSpan::new(4, 1));

        let mut a = CounterSpan::new(8, 9);
        let b = CounterSpan::new(9, 8);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, CounterSpan::new(8, 10));

        let a = CounterSpan::new(8, 9);
        let b = CounterSpan::new(10, 11);
        assert!(!a.is_mergable(&b, &()));

        let mut a = CounterSpan::new(0, 2);
        let b = CounterSpan::new(2, 4);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, CounterSpan::new(0, 4));

        let mut a = CounterSpan::new(4, 2);
        let b = CounterSpan::new(2, 0);
        assert!(a.is_mergable(&b, &()));
        a.merge(&b, &());
        assert_eq!(a, CounterSpan::new(4, 0));
    }
}
