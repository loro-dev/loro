use crate::id::{ClientID, Counter, ID};
use rle::{HasLength, Mergable, Slice, Sliceable};

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
    pub fn reverse(&mut self) {
        if self.from == self.to {
            return;
        }

        if self.from < self.to {
            (self.from, self.to) = (self.to - 1, self.from - 1);
        } else {
            (self.from, self.to) = (self.to + 1, self.from + 1);
        }
    }

    #[inline]
    pub fn min(&self) -> Counter {
        if self.from < self.to {
            self.from
        } else {
            self.to
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

    fn end(&self) -> i32 {
        if self.from > self.to {
            self.from + 1
        } else {
            self.to
        }
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
    pub fn new(client_id: ClientID, from: Counter, to: Counter) -> Self {
        Self {
            client_id,
            counter: CounterSpan { from, to },
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
    fn id_end(&self) -> ID {
        self.id_start().inc(self.len() as i32)
    }

    fn id_last(&self) -> ID {
        self.id_start().inc(self.len() as i32 - 1)
    }
}
impl<T: HasId + HasLength> HasIdSpan for T {}

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
