use crate::id::{ClientID, Counter, ID};
use rle::{HasLength, Mergable, Slice, Sliceable};

/// [from, to)
/// this is different from [std::ops::Range] because `from` may be greater than `to`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CounterSpan {
    pub from: Counter,
    pub to: Counter,
}

impl CounterSpan {
    #[inline]
    pub fn new(from: Counter, to: Counter) -> Self {
        CounterSpan { from, to }
    }

    #[inline]
    pub fn min(&self) -> Counter {
        if self.from < self.to {
            self.from
        } else {
            self.to + 1
        }
    }

    #[inline]
    pub fn max(&self) -> Counter {
        if self.from > self.to {
            self.from
        } else {
            self.to - 1
        }
    }

    #[inline]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let min = self.min().max(other.min());
        let max = self.max().min(other.max());
        if min <= max {
            Some(CounterSpan::new(min, max))
        } else {
            None
        }
    }

    #[inline]
    pub fn does_intersect(&self, other: &Self) -> bool {
        let min = self.min().max(other.min());
        let max = self.max().min(other.max());
        min <= max
    }
}

impl HasLength for CounterSpan {
    #[inline]
    fn len(&self) -> usize {
        if self.to > self.from {
            (self.to - self.from) as usize
        } else {
            (self.from - self.to) as usize
        }
    }
}

impl Sliceable for CounterSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from <= to);
        let len = to - from;
        assert!(len <= self.len());
        if self.from < self.to {
            CounterSpan {
                from: self.from + from as Counter,
                to: self.from + to as Counter,
            }
        } else {
            CounterSpan {
                from: self.from - from as Counter,
                to: self.from - to as Counter,
            }
        }
    }
}

impl Mergable for CounterSpan {
    #[inline]
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        self.to == other.from
    }

    #[inline]
    fn merge(&mut self, other: &Self, _: &()) {
        self.to = other.to;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdSpan {
    pub client_id: ClientID,
    pub counter: CounterSpan,
}

impl IdSpan {
    #[inline]
    pub fn min(&self) -> Counter {
        self.counter.min()
    }

    #[inline]
    pub fn max(&self) -> Counter {
        self.counter.max()
    }

    #[inline]
    pub fn does_intersect(&self, other: &Self) -> bool {
        self.client_id == other.client_id && self.counter.does_intersect(&other.counter)
    }

    #[inline]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        if self.client_id != other.client_id {
            None
        } else {
            Some(IdSpan {
                client_id: self.client_id,
                counter: self.counter.intersect(&other.counter)?,
            })
        }
    }

    #[inline]
    pub fn start(&self) -> ID {
        ID::new(self.client_id, self.counter.min())
    }

    #[inline]
    pub fn end(&self) -> ID {
        ID::new(self.client_id, self.counter.max())
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
