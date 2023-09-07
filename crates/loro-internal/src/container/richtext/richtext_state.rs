use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use generic_btree::{
    rle::{insert_with_split, HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, LengthFinder, Query, UseLengthFinder,
};
use loro_common::{Counter, LoroValue, PeerID, ID};
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    ops::{Add, AddAssign, Range, RangeBounds, Sub},
    str::Utf8Error,
    sync::Arc,
};

use crate::{
    change::Lamport, container::text::utf16::count_utf16_chars, InternalString, VersionVector,
};

use self::query::{EntityQueryT, UnicodeQuery};

use super::{
    query_by_len::{IndexQuery, QueryByLen},
    tinyvec::TinyVec,
    RichtextSpan, StyleInner, TextStyleInfo,
};

#[derive(Clone, Debug)]
pub(crate) struct RichtextState {
    tree: BTree<RichtextTreeTrait>,
}

#[derive(Clone, Debug)]
enum Elem {
    Text { unicode_len: i32, text: BytesSlice },
    Style(TinyVec<TextStyleInfo, 16>),
}

impl Elem {
    pub fn try_from_bytes(s: BytesSlice) -> Result<Self, Utf8Error> {
        Ok(Elem::Text {
            unicode_len: std::str::from_utf8(&s)?.chars().count() as i32,
            text: s,
        })
    }
}

impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        match self {
            Elem::Text { unicode_len, text } => *unicode_len as usize,
            Elem::Style(data) => data.len(),
        }
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Elem::Text { text: l, .. }, Elem::Text { text: r, .. }) => l.can_merge(r),
            (Elem::Style(l), Elem::Style(r)) => l.can_merge(r),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (
                Elem::Text { unicode_len, text },
                Elem::Text {
                    unicode_len: rhs_len,
                    text: rhs_text,
                },
            ) => {
                *unicode_len += *rhs_len;
                text.try_merge(rhs_text).unwrap();
            }
            (Elem::Style(a), Elem::Style(b)) => {
                a.merge(&b);
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self, left) {
            (
                Elem::Text { unicode_len, text },
                Elem::Text {
                    unicode_len: left_len,
                    text: left_text,
                },
            ) => {
                *unicode_len += *left_len;
                // TODO: small PERF improvement
                let mut new_text = left_text.clone();
                new_text.try_merge(text);
                *text = new_text;
            }
            (Elem::Style(a), Elem::Style(b)) => {
                a.merge_left(&b);
            }
            _ => unreachable!(),
        }
    }
}

impl Sliceable for Elem {
    fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let start_index = match range.start_bound() {
            std::ops::Bound::Included(s) => *s,
            std::ops::Bound::Excluded(s) => *s + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end_index = match range.end_bound() {
            std::ops::Bound::Included(s) => *s + 1,
            std::ops::Bound::Excluded(s) => *s,
            std::ops::Bound::Unbounded => self.rle_len(),
        };

        let Elem::Text { unicode_len, text } = self else {
            return self.slice(start_index..end_index);
        };

        let s = std::str::from_utf8(text).unwrap();
        let from = unicode_to_byte_index(s, start_index).unwrap();
        let len = unicode_to_byte_index(&s[from..], end_index - start_index).unwrap();
        let to = from + len;
        return Elem::Text {
            unicode_len: (end_index - start_index) as i32,
            text: text.slice_clone(from..to),
        };
    }
}

fn unicode_to_byte_index(s: &str, unicode_index: usize) -> Option<usize> {
    let mut current_unicode_index = 0;
    for (byte_index, _) in s.char_indices() {
        if current_unicode_index == unicode_index {
            return Some(byte_index);
        }
        current_unicode_index += 1;
    }
    None
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
struct Cache {
    unicode_len: i32,
    utf16_len: i32,
    entity_len: i32,
}

impl AddAssign for Cache {
    fn add_assign(&mut self, rhs: Self) {
        self.unicode_len += rhs.unicode_len;
        self.utf16_len += rhs.utf16_len;
        self.entity_len += rhs.entity_len;
    }
}

impl Add for Cache {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            unicode_len: self.unicode_len + rhs.unicode_len,
            utf16_len: self.utf16_len + rhs.utf16_len,
            entity_len: self.entity_len + rhs.entity_len,
        }
    }
}

impl Sub for Cache {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            unicode_len: self.unicode_len - rhs.unicode_len,
            utf16_len: self.utf16_len - rhs.utf16_len,
            entity_len: self.entity_len - rhs.entity_len,
        }
    }
}

struct RichtextTreeTrait;

impl BTreeTrait for RichtextTreeTrait {
    type Elem = Elem;

    type Cache = Cache;

    type CacheDiff = Cache;

    const MAX_LEN: usize = 16;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
        diff: Option<Self::CacheDiff>,
    ) -> Option<Self::CacheDiff> {
        match diff {
            Some(diff) => {
                *cache += diff;
                Some(diff)
            }
            None => {
                let mut new_cache = Cache::default();
                for child in caches {
                    new_cache += child.cache;
                }

                let diff = new_cache - *cache;
                *cache = new_cache;
                Some(diff)
            }
        }
    }

    fn calc_cache_leaf(
        cache: &mut Self::Cache,
        elements: &[Self::Elem],
        diff: Option<Self::CacheDiff>,
    ) -> Self::CacheDiff {
        match diff {
            Some(diff) => {
                *cache += diff;
                diff
            }
            None => {
                let mut new_cache = Cache::default();
                for elem in elements {
                    match elem {
                        Elem::Text { unicode_len, .. } => {
                            new_cache.unicode_len += unicode_len;
                            new_cache.utf16_len += unicode_len;
                            new_cache.entity_len += 1;
                        }
                        Elem::Style(size) => new_cache.entity_len += size.len() as i32,
                    }
                }

                let diff = new_cache - *cache;
                *cache = new_cache;
                diff
            }
        }
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += *diff2;
    }

    fn insert(
        elements: &mut generic_btree::HeapVec<Self::Elem>,
        index: usize,
        offset: usize,
        elem: Self::Elem,
    ) {
        insert_with_split(elements, index, offset, elem)
    }
}

// This query implementation will prefer right element when both left element and right element are valid.
mod query {
    use super::*;

    pub(super) struct UnicodeQueryT;
    pub(super) type UnicodeQuery = IndexQuery<UnicodeQueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for UnicodeQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.unicode_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                Elem::Text { unicode_len, text } => *unicode_len as usize,
                Elem::Style(_) => 0,
            }
        }
    }

    pub(super) struct Utf16QueryT;
    pub(super) type Utf16Query = IndexQuery<Utf16QueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for Utf16QueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.utf16_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                Elem::Text {
                    unicode_len: _,
                    text,
                } => count_utf16_chars(text),
                Elem::Style(_) => 0,
            }
        }
    }

    pub(super) struct EntityQueryT;
    pub(super) type EntityQuery = IndexQuery<EntityQueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for EntityQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.entity_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                Elem::Text {
                    unicode_len,
                    text: _,
                } => *unicode_len as usize,
                Elem::Style(data) => data.len(),
            }
        }
    }
}

impl RichtextState {
    /// Insert text at a unicode index. Return the entity index.
    pub(crate) fn insert(&mut self, pos: usize, text: BytesSlice) -> usize {
        let right = self.tree.query::<UnicodeQuery>(&pos);
        let entity_index = self.get_entity_index_from_path(right);

        // TODO: find the best insert position
        let insert_pos = right;
        self.tree
            .insert_by_query_result(insert_pos, Elem::try_from_bytes(text).unwrap());

        entity_index
    }

    fn get_entity_index_from_path(&mut self, right: generic_btree::QueryResult) -> usize {
        let mut entity_index = 0;
        self.tree.visit_previous_caches(right, |cache| match cache {
            generic_btree::PreviousCache::NodeCache(cache) => {
                entity_index += EntityQueryT::get_cache_len(cache);
            }
            generic_btree::PreviousCache::PrevSiblingElem(elem) => {
                entity_index += EntityQueryT::get_elem_len(elem);
            }
            generic_btree::PreviousCache::ThisElemAndOffset { elem: _, offset } => {
                entity_index += offset;
            }
        });
        entity_index
    }

    /// Delete a range of text.
    ///
    /// Delete a range of text. (The style anchors included in the range are not deleted.)
    pub(crate) fn delete(&mut self, pos: usize, len: usize) {
        let mut style_anchors: Vec<Elem> = Vec::new();
        let mut removed_entity_ranges: Vec<Range<usize>> = Vec::new();
        let q = self.tree.query::<UnicodeQuery>(&pos);
        let mut entity_index = self.get_entity_index_from_path(q);

        for span in self.tree.drain::<UnicodeQuery>(pos..pos + len) {
            match span {
                Elem::Style(style) => {
                    entity_index += style.len();
                    if let Some(last) = style_anchors.last_mut() {
                        let Elem::Style(last) = last else {
                            unreachable!()
                        };
                        if last.can_merge(&style) {
                            last.merge(&style);
                        } else {
                            style_anchors.push(Elem::Style(style));
                        }
                    } else {
                        style_anchors.push(Elem::Style(style));
                    }
                }
                Elem::Text {
                    unicode_len,
                    text: _,
                } => {
                    if let Some(last) = removed_entity_ranges.last_mut() {
                        if last.end == entity_index {
                            last.end += unicode_len as usize;
                        } else {
                            removed_entity_ranges
                                .push(entity_index..(entity_index + unicode_len as usize));
                        }
                    } else {
                        removed_entity_ranges
                            .push(entity_index..(entity_index + unicode_len as usize));
                    }

                    entity_index += unicode_len as usize;
                }
            }
        }

        let q = self.tree.query::<UnicodeQuery>(&pos);
        self.tree.insert_many_by_query_result(&q, style_anchors);
        todo!("delete ranges in style range map");
    }

    /// Mark a range of text with a style.
    ///
    /// Return the entity index ranges.
    pub(crate) fn mark(&mut self, range: Range<usize>, style: Arc<StyleInner>) -> Range<usize> {
        todo!()
    }

    pub fn iter(&self) -> impl Iterator<Item = RichtextSpan<'_>> {
        None.into_iter()
    }

    pub fn to_vec(&self) -> Vec<RichtextSpan<'_>> {
        self.iter().collect()
    }
}
