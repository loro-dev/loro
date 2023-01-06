use std::{
    cmp::Ordering,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use fxhash::FxHashMap;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use tracing::instrument;

use crate::{
    change::Lamport,
    id::{Counter, ID},
    span::{CounterSpan, HasId, HasIdSpan, IdSpan},
    ClientID, LoroError,
};

/// [VersionVector](https://en.wikipedia.org/wiki/Version_vector)
///
/// It's a map from [ClientID] to [Counter]. Its a right-open interval.
/// i.e. a [VersionVector] of `{A: 1, B: 2}` means that A has 1 atomic op and B has 2 atomic ops,
/// thus ID of `{client: A, counter: 1}` is out of the range.
///
/// In implementation, it's a immutable hash map with O(1) clone. Because
/// - we want a cheap clone op on vv;
/// - neighbor op's VersionVectors are very similar, most of the memory can be shared in
/// immutable hashmap
///
/// see also [im].
#[repr(transparent)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionVector(FxHashMap<ClientID, Counter>);

// TODO: use new type
pub type Frontiers = SmallVec<[ID; 2]>;

impl PartialEq for VersionVector {
    fn eq(&self, other: &Self) -> bool {
        self.iter()
            .all(|(client, counter)| other.get(client).unwrap_or(&0) == counter)
            && other
                .iter()
                .all(|(client, counter)| self.get(client).unwrap_or(&0) == counter)
    }
}

impl Eq for VersionVector {}

impl Deref for VersionVector {
    type Target = FxHashMap<ClientID, Counter>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// TODO: wrap this type?
pub type IdSpanVector = FxHashMap<ClientID, CounterSpan>;

impl HasId for (&ClientID, &CounterSpan) {
    fn id_start(&self) -> ID {
        ID {
            client_id: *self.0,
            counter: self.1.min(),
        }
    }
}

impl HasId for (ClientID, CounterSpan) {
    fn id_start(&self) -> ID {
        ID {
            client_id: self.0,
            counter: self.1.min(),
        }
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct VersionVectorDiff {
    /// need to add these spans to move from right to left
    pub left: IdSpanVector,
    /// need to add these spans to move from left to right
    pub right: IdSpanVector,
}

impl VersionVectorDiff {
    #[inline]
    pub fn merge_left(&mut self, span: IdSpan) {
        merge(&mut self.left, span);
    }

    #[inline]
    pub fn merge_right(&mut self, span: IdSpan) {
        merge(&mut self.right, span);
    }

    #[inline]
    pub fn subtract_start_left(&mut self, span: IdSpan) {
        subtract_start(&mut self.left, span);
    }

    #[inline]
    pub fn subtract_start_right(&mut self, span: IdSpan) {
        subtract_start(&mut self.right, span);
    }

    pub fn get_id_spans_left(&self) -> impl Iterator<Item = IdSpan> + '_ {
        self.left.iter().map(|(client_id, span)| IdSpan {
            client_id: *client_id,
            counter: *span,
        })
    }

    pub fn get_id_spans_right(&self) -> impl Iterator<Item = IdSpan> + '_ {
        self.right.iter().map(|(client_id, span)| IdSpan {
            client_id: *client_id,
            counter: *span,
        })
    }
}

fn subtract_start(m: &mut FxHashMap<ClientID, CounterSpan>, target: IdSpan) {
    if let Some(span) = m.get_mut(&target.client_id) {
        if span.start < target.counter.end {
            span.start = target.counter.end;
        }
    }
}

fn merge(m: &mut FxHashMap<ClientID, CounterSpan>, mut target: IdSpan) {
    target.normalize_();
    if let Some(span) = m.get_mut(&target.client_id) {
        span.start = span.start.min(target.counter.start);
        span.end = span.end.max(target.counter.end);
    } else {
        m.insert(target.client_id, target.counter);
    }
}

impl PartialOrd for VersionVector {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let mut self_greater = true;
        let mut other_greater = true;
        let mut eq = true;
        for (client_id, other_end) in other.iter() {
            if let Some(self_end) = self.get(client_id) {
                if self_end < other_end {
                    self_greater = false;
                    eq = false;
                }
                if self_end > other_end {
                    other_greater = false;
                    eq = false;
                }
            } else {
                self_greater = false;
                eq = false;
            }
        }

        for (client_id, _) in self.iter() {
            if other.contains_key(client_id) {
                continue;
            } else {
                other_greater = false;
                eq = false;
            }
        }

        if eq {
            Some(Ordering::Equal)
        } else if self_greater {
            Some(Ordering::Greater)
        } else if other_greater {
            Some(Ordering::Less)
        } else {
            None
        }
    }
}

impl DerefMut for VersionVector {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl VersionVector {
    pub fn diff(&self, rhs: &Self) -> VersionVectorDiff {
        let mut ans: VersionVectorDiff = Default::default();
        for (client_id, &counter) in self.iter() {
            if let Some(&rhs_counter) = rhs.get(client_id) {
                match counter.cmp(&rhs_counter) {
                    Ordering::Less => {
                        ans.right.insert(
                            *client_id,
                            CounterSpan {
                                start: counter,
                                end: rhs_counter,
                            },
                        );
                    }
                    Ordering::Greater => {
                        ans.left.insert(
                            *client_id,
                            CounterSpan {
                                start: rhs_counter,
                                end: counter,
                            },
                        );
                    }
                    Ordering::Equal => {}
                }
            } else {
                ans.left.insert(
                    *client_id,
                    CounterSpan {
                        start: 0,
                        end: counter,
                    },
                );
            }
        }
        for (client_id, &rhs_counter) in rhs.iter() {
            if !self.contains_key(client_id) {
                ans.right.insert(
                    *client_id,
                    CounterSpan {
                        start: 0,
                        end: rhs_counter,
                    },
                );
            }
        }

        ans
    }

    /// Returns two iterators that cover the differences between two version vectors.
    ///
    /// - The first iterator contains the spans that are in `self` but not in `rhs`
    /// - The second iterator contains the spans that are in `rhs` but not in `self`
    pub fn diff_iter<'a>(
        &'a self,
        rhs: &'a Self,
    ) -> (
        impl Iterator<Item = IdSpan> + 'a,
        impl Iterator<Item = IdSpan> + 'a,
    ) {
        (self.sub_iter(rhs), rhs.sub_iter(self))
    }

    /// Returns the spans that are in `self` but not in `rhs`
    pub fn sub_iter<'a>(&'a self, rhs: &'a Self) -> impl Iterator<Item = IdSpan> + 'a {
        self.iter().filter_map(move |(client_id, &counter)| {
            if let Some(&rhs_counter) = rhs.get(client_id) {
                if counter > rhs_counter {
                    Some(IdSpan {
                        client_id: *client_id,
                        counter: CounterSpan {
                            start: rhs_counter,
                            end: counter,
                        },
                    })
                } else {
                    None
                }
            } else {
                Some(IdSpan {
                    client_id: *client_id,
                    counter: CounterSpan {
                        start: 0,
                        end: counter,
                    },
                })
            }
        })
    }

    pub fn sub_vec(&self, rhs: &Self) -> IdSpanVector {
        self.sub_iter(rhs)
            .map(|x| (x.client_id, x.counter))
            .collect()
    }

    pub fn to_spans(&self) -> IdSpanVector {
        self.iter()
            .map(|(client_id, &counter)| {
                (
                    *client_id,
                    CounterSpan {
                        start: 0,
                        end: counter,
                    },
                )
            })
            .collect()
    }

    #[inline]
    pub fn get_frontiers(&self) -> SmallVec<[ID; 2]> {
        self.iter()
            .filter_map(|(client_id, &counter)| {
                if counter > 0 {
                    Some(ID {
                        client_id: *client_id,
                        counter: counter - 1,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    #[inline]
    pub fn new() -> Self {
        Self(Default::default())
    }

    /// set the inclusive ending point. target id will be included by self
    #[inline]
    pub fn set_last(&mut self, id: ID) {
        self.0.insert(id.client_id, id.counter + 1);
    }

    #[inline]
    pub fn get_last(&mut self, client_id: ClientID) -> Option<Counter> {
        self.0
            .get(&client_id)
            .and_then(|&x| if x == 0 { None } else { Some(x - 1) })
    }

    /// set the exclusive ending point. target id will NOT be included by self
    #[inline]
    pub fn set_end(&mut self, id: ID) {
        self.0.insert(id.client_id, id.counter);
    }

    /// Update the end counter of the given client if the end is greater.
    /// Return whether updated
    #[inline]
    pub fn try_update_last(&mut self, id: ID) -> bool {
        if let Some(end) = self.0.get_mut(&id.client_id) {
            if *end < id.counter + 1 {
                *end = id.counter + 1;
                true
            } else {
                false
            }
        } else {
            self.0.insert(id.client_id, id.counter + 1);
            true
        }
    }

    pub fn get_missing_span(&self, target: &Self) -> Vec<IdSpan> {
        let mut ans = vec![];
        for (client_id, other_end) in target.iter() {
            if let Some(my_end) = self.get(client_id) {
                if my_end < other_end {
                    ans.push(IdSpan::new(*client_id, *my_end, *other_end));
                }
            } else {
                ans.push(IdSpan::new(*client_id, 0, *other_end));
            }
        }

        ans
    }

    pub fn merge(&mut self, other: &Self) {
        for (&client_id, &other_end) in other.iter() {
            if let Some(my_end) = self.get_mut(&client_id) {
                if *my_end < other_end {
                    *my_end = other_end;
                }
            } else {
                self.0.insert(client_id, other_end);
            }
        }
    }

    pub fn includes_id(&self, id: ID) -> bool {
        if let Some(end) = self.get(&id.client_id) {
            if *end > id.counter {
                return true;
            }
        }
        false
    }

    pub fn intersect_span<S: HasIdSpan>(&self, target: &S) -> Option<CounterSpan> {
        let id = target.id_start();
        if let Some(end) = self.get(&id.client_id) {
            if *end > id.counter {
                return Some(CounterSpan {
                    start: id.counter,
                    end: *end,
                });
            }
        }

        None
    }

    pub fn extend_to_include_vv(&mut self, vv: &VersionVector) {
        for (&client_id, &counter) in vv.iter() {
            if let Some(my_counter) = self.get_mut(&client_id) {
                if *my_counter < counter {
                    *my_counter = counter;
                }
            } else {
                self.0.insert(client_id, counter);
            }
        }
    }

    pub fn extend_to_include_last_id(&mut self, id: ID) {
        if let Some(counter) = self.get_mut(&id.client_id) {
            if *counter <= id.counter {
                *counter = id.counter + 1;
            }
        } else {
            self.set_last(id)
        }
    }

    pub fn extend_to_include(&mut self, span: IdSpan) {
        if let Some(counter) = self.get_mut(&span.client_id) {
            if *counter < span.counter.end() {
                *counter = span.counter.end();
            }
        } else {
            self.insert(span.client_id, span.counter.end());
        }
    }

    pub fn shrink_to_exclude(&mut self, span: IdSpan) {
        if span.counter.min() == 0 {
            self.remove(&span.client_id);
            return;
        }

        if let Some(counter) = self.get_mut(&span.client_id) {
            if *counter > span.counter.min() {
                *counter = span.counter.min();
            }
        }
    }

    pub fn forward(&mut self, spans: &IdSpanVector) {
        for span in spans.iter() {
            self.extend_to_include(IdSpan {
                client_id: *span.0,
                counter: *span.1,
            });
        }
    }

    pub fn retreat(&mut self, spans: &IdSpanVector) {
        for span in spans.iter() {
            self.shrink_to_exclude(IdSpan {
                client_id: *span.0,
                counter: *span.1,
            });
        }
    }

    pub fn intersection(&self, other: &VersionVector) -> VersionVector {
        let mut ans = VersionVector::new();
        for (client_id, &counter) in self.iter() {
            if let Some(&other_counter) = other.get(client_id) {
                if counter < other_counter {
                    if counter != 0 {
                        ans.insert(*client_id, counter);
                    }
                } else if other_counter != 0 {
                    ans.insert(*client_id, other_counter);
                }
            }
        }
        ans
    }

    #[inline(always)]
    #[instrument(skip_all)]
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    #[inline(always)]
    #[instrument(skip_all)]
    pub fn decode(bytes: &[u8]) -> Result<Self, LoroError> {
        postcard::from_bytes(bytes).map_err(|_| LoroError::DecodeVersionVectorError)
    }
}

impl Default for VersionVector {
    fn default() -> Self {
        Self::new()
    }
}

impl From<FxHashMap<ClientID, Counter>> for VersionVector {
    fn from(map: FxHashMap<ClientID, Counter>) -> Self {
        let mut im_map = FxHashMap::default();
        for (client_id, counter) in map {
            im_map.insert(client_id, counter);
        }
        Self(im_map)
    }
}

impl From<Vec<ID>> for VersionVector {
    fn from(vec: Vec<ID>) -> Self {
        let mut vv = VersionVector::new();
        for id in vec {
            vv.set_last(id);
        }

        vv
    }
}

impl FromIterator<ID> for VersionVector {
    fn from_iter<T: IntoIterator<Item = ID>>(iter: T) -> Self {
        let mut vv = VersionVector::new();
        for id in iter {
            vv.set_last(id);
        }

        vv
    }
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct TotalOrderStamp {
    pub(crate) lamport: Lamport,
    pub(crate) client_id: ClientID,
}

pub fn are_frontiers_eq(a: &[ID], b: &[ID]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut a: SmallVec<[ID; 10]> = a.into();
    let mut b: SmallVec<[ID; 10]> = b.into();

    a.sort();
    b.sort();

    a == b
}

#[derive(Debug, Default)]
pub struct PatchedVersionVector {
    pub base: Arc<VersionVector>,
    pub patch: VersionVector,
}

impl Clone for PatchedVersionVector {
    fn clone(&self) -> Self {
        Self {
            base: Arc::clone(&self.base),
            patch: self.patch.clone(),
        }
    }
}

impl From<PatchedVersionVector> for VersionVector {
    fn from(mut v: PatchedVersionVector) -> Self {
        for (client_id, counter) in v.base.iter() {
            if v.patch.contains_key(client_id) {
                continue;
            }

            v.patch.set_last(ID::new(*client_id, *counter));
        }

        v.patch
    }
}

impl PatchedVersionVector {
    #[inline]
    pub fn new(base: Arc<VersionVector>) -> Self {
        Self {
            base,
            patch: Default::default(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.patch.is_empty() && self.base.is_empty()
    }

    pub fn from_version(base: &Arc<VersionVector>, version: &VersionVector) -> Self {
        let mut patch = VersionVector::new();
        for (client_id, counter) in version.iter() {
            if let Some(base_counter) = base.get(client_id) {
                if *base_counter != *counter {
                    patch.set_end(ID::new(*client_id, *counter));
                }
            } else {
                patch.set_end(ID::new(*client_id, *counter));
            }
        }

        if cfg!(debug_assertions) {
            for (client_id, counter) in base.iter() {
                if let Some(patch_counter) = version.get(client_id) {
                    assert!(*patch_counter >= *counter);
                } else {
                    unreachable!("base should be a subset of version");
                }
            }
        }

        Self {
            base: Arc::clone(base),
            patch,
        }
    }

    #[inline]
    pub fn extend_to_include_last_id(&mut self, id: ID) {
        self.patch.extend_to_include_last_id(id);
        self.omit_if_needless(id.client_id);
    }

    #[inline]
    pub fn set_end(&mut self, id: ID) {
        self.patch.set_end(id);
        self.omit_if_needless(id.client_id);
    }

    #[inline]
    pub fn set_last(&mut self, id: ID) {
        self.patch.set_last(id);
        self.omit_if_needless(id.client_id);
    }

    #[inline]
    pub fn extend_to_include(&mut self, span: IdSpan) {
        self.patch.extend_to_include(span);
        self.omit_if_needless(span.client_id);
    }

    #[inline]
    pub fn shrink_to_exclude(&mut self, span: IdSpan) {
        self.patch.shrink_to_exclude(span);
        self.omit_if_needless(span.client_id);
    }

    #[inline]
    pub fn forward(&mut self, spans: &IdSpanVector) {
        for span in spans.iter() {
            let span = IdSpan {
                client_id: *span.0,
                counter: *span.1,
            };

            if let Some(counter) = self.patch.get_mut(&span.client_id) {
                if *counter < span.counter.end() {
                    *counter = span.counter.end();
                    self.omit_if_needless(span.client_id);
                }
            } else {
                let target = span.counter.end();
                if self.base.get(&span.client_id) == Some(&target) {
                    continue;
                }

                self.patch.insert(span.client_id, target);
            }
        }
    }

    #[inline]
    pub fn retreat(&mut self, spans: &IdSpanVector) {
        for span in spans.iter() {
            let span = IdSpan {
                client_id: *span.0,
                counter: *span.1,
            };

            if let Some(counter) = self.patch.get_mut(&span.client_id) {
                if *counter > span.counter.min() {
                    *counter = span.counter.min();
                    self.omit_if_needless(span.client_id);
                }
            }
        }
    }

    #[inline(always)]
    fn omit_if_needless(&mut self, client_id: ClientID) {
        if let Some(patch_value) = self.patch.get(&client_id) {
            if *patch_value == *self.base.get(&client_id).unwrap_or(&0) {
                self.patch.remove(&client_id);
            }
        }
    }

    #[inline]
    pub fn get(&self, client_id: &ClientID) -> Option<&Counter> {
        self.patch
            .get(client_id)
            .or_else(|| self.base.get(client_id))
    }

    #[inline]
    pub fn insert(&mut self, client_id: ClientID, counter: Counter) {
        self.patch.insert(client_id, counter);
        self.omit_if_needless(client_id);
    }

    #[inline]
    pub fn includes_id(&self, id: ID) -> bool {
        self.patch.includes_id(id) || self.base.includes_id(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ClientID, &Counter)> {
        self.patch.iter().chain(
            self.base
                .iter()
                .filter(|(client_id, _)| !self.patch.contains_key(client_id)),
        )
    }

    pub fn sub_iter<'a>(&'a self, rhs: &'a Self) -> impl Iterator<Item = IdSpan> + 'a {
        if !Arc::ptr_eq(&self.base, &rhs.base) {
            unimplemented!();
        }

        self.patch.sub_iter(&rhs.patch)
    }

    pub fn diff_iter<'a>(
        &'a self,
        rhs: &'a Self,
    ) -> (
        impl Iterator<Item = IdSpan> + 'a,
        impl Iterator<Item = IdSpan> + 'a,
    ) {
        if !Arc::ptr_eq(&self.base, &rhs.base) {
            unimplemented!();
        }

        self.patch.diff_iter(&rhs.patch)
    }
}

impl PartialEq for PatchedVersionVector {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.base, &other.base) {
            self.patch.eq(&other.patch)
        } else {
            unimplemented!()
        }
    }
}

impl PartialOrd for PatchedVersionVector {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if Arc::ptr_eq(&self.base, &other.base) {
            self.patch.partial_cmp(&other.patch)
        } else {
            unimplemented!()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod cmp {
        use super::*;
        #[test]
        fn test() {
            let a: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Equal));

            let a: VersionVector = vec![ID::new(1, 2), ID::new(2, 1)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), None);

            let a: VersionVector = vec![ID::new(1, 2), ID::new(2, 3)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Greater));

            let a: VersionVector = vec![ID::new(1, 0), ID::new(2, 2)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Less));
        }
    }

    #[test]
    fn im() {
        let mut a = VersionVector::new();
        a.set_last(ID::new(1, 1));
        a.set_last(ID::new(2, 1));
        let mut b = a.clone();
        b.merge(&vec![ID::new(1, 2), ID::new(2, 2)].into());
        assert!(a != b);
        assert_eq!(a.get(&1), Some(&2));
        assert_eq!(a.get(&2), Some(&2));
        assert_eq!(b.get(&1), Some(&3));
        assert_eq!(b.get(&2), Some(&3));
    }

    #[test]
    fn field_order() {
        let tos = TotalOrderStamp {
            lamport: 0,
            client_id: 1,
        };
        let buf = vec![0, 1];
        assert_eq!(postcard::from_bytes::<TotalOrderStamp>(&buf).unwrap(), tos);
    }
}
