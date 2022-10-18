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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CounterSpan {
    // TODO: should be private. user should not be able to change start from smaller than end to be greater than end
    pub start: Counter,
    // TODO: should be private
    pub end: Counter,
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

    #[inline]
    pub fn min(&self) -> Counter {
        if self.start < self.end {
            self.start
        } else {
            self.end
        }
    }

    #[inline]
    pub fn max(&self) -> Counter {
        if self.start > self.end {
            self.start
        } else {
            self.end - 1
        }
    }

    pub fn end(&self) -> i32 {
        if self.start > self.end {
            self.start + 1
        } else {
            self.end
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

    pub fn set_start(&mut self, start: Counter) {
        if self.start < self.end {
            self.start = start.min(self.end);
        } else {
            self.start = start.max(self.end);
        }
    }

    pub fn set_end(&mut self, end: Counter) {
        if self.start < self.end {
            self.end = end.max(self.start);
        } else {
            self.end = end.min(self.start);
        }
    }
}

impl HasLength for CounterSpan {
    #[inline]
    fn len(&self) -> usize {
        if self.end > self.start {
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
        assert!(len <= self.len());
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
        self.end == other.start
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

    #[inline]
    pub fn min_ctr(&self) -> Counter {
        self.counter.min()
    }

    #[inline]
    pub fn max_ctr(&self) -> Counter {
        self.counter.max()
    }

    #[inline]
    pub fn min_id(&self) -> ID {
        ID::new(self.client_id, self.counter.min())
    }

    #[inline]
    pub fn max_id(&self) -> ID {
        ID::new(self.client_id, self.counter.max())
    }

    #[inline]
    pub fn end_id(&self) -> ID {
        ID::new(self.client_id, self.counter.end())
    }
}

impl HasLength for IdSpan {
    #[inline]
    fn len(&self) -> usize {
        self.counter.len()
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

pub trait HasIdSpan: HasId + HasLength {
    fn id_span(&self) -> IdSpan {
        let id = self.id_start();
        IdSpan::new(id.client_id, id.counter, id.counter + self.len() as Counter)
    }

    fn id_end(&self) -> ID {
        self.id_start().inc(self.len() as i32)
    }

    fn id_last(&self) -> ID {
        self.id_start().inc(self.len() as i32 - 1)
    }

    fn contains_id(&self, id: ID) -> bool {
        let id_start = self.id_start();
        if id.client_id != id_start.client_id {
            return false;
        }

        id_start.counter <= id.counter && id.counter < id_start.counter + self.len() as Counter
    }
}
impl<T: HasId + HasLength> HasIdSpan for T {}

pub trait HasLamport {
    fn lamport(&self) -> Lamport;
}

pub trait HasLamportSpan: HasLamport + HasLength {
    fn lamport_end(&self) -> Lamport {
        self.lamport() + self.len() as Lamport
    }

    fn lamport_last(&self) -> Lamport {
        self.lamport() + self.len() as Lamport - 1
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
    use rle::RleVec;

    use super::*;

    macro_rules! id_spans {
        ($([$client_id:expr, $from:expr, $to:expr]),*) => {
            {
                let mut id_spans = RleVec::new();
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
        let mut id_span_vec = RleVec::new();
        id_span_vec.push(IdSpan {
            client_id: 0,
            counter: CounterSpan::new(0, 2),
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.len(), 2);
        id_span_vec.push(IdSpan {
            client_id: 0,
            counter: CounterSpan::new(2, 4),
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.len(), 4);
        id_span_vec.push(IdSpan {
            client_id: 2,
            counter: CounterSpan::new(2, 4),
        });
        assert_eq!(id_span_vec.merged_len(), 2);
        assert_eq!(id_span_vec.len(), 6);
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
