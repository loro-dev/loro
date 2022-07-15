use crate::id::{ClientID, ID};
use rle::{HasLength, Mergable, Slice, Sliceable};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdSpan {
    pub client_id: ClientID,
    pub from: usize,
    pub to: usize,
}

impl IdSpan {
    #[inline]
    pub fn min(&self) -> usize {
        if self.from < self.to {
            self.from
        } else {
            self.to
        }
    }

    #[inline]
    pub fn max(&self) -> usize {
        if self.from > self.to {
            self.from
        } else {
            self.to
        }
    }
}

impl HasLength for IdSpan {
    fn len(&self) -> usize {
        if self.to > self.from {
            self.to - self.from
        } else {
            self.from - self.to
        }
    }
}

impl Sliceable for IdSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from <= to);
        let len = to - from;
        assert!(len <= self.len());
        if self.from < self.to {
            IdSpan {
                client_id: self.client_id,
                from: self.from + from,
                to: self.from + to,
            }
        } else {
            IdSpan {
                client_id: self.client_id,
                from: self.from - from,
                to: self.from - to,
            }
        }
    }
}

impl Mergable for IdSpan {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        self.client_id == other.client_id && self.to == other.from
    }

    fn merge(&mut self, other: &Self, _: &()) {
        self.to = other.to;
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
                        from: $from,
                        to: $to,
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
            from: 0,
            to: 2,
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.len(), 2);
        id_span_vec.push(IdSpan {
            client_id: 0,
            from: 2,
            to: 4,
        });
        assert_eq!(id_span_vec.merged_len(), 1);
        assert_eq!(id_span_vec.len(), 4);
        id_span_vec.push(IdSpan {
            client_id: 2,
            from: 2,
            to: 4,
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
