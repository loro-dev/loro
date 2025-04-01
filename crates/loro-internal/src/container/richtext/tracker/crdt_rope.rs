use std::cmp::Ordering;

use generic_btree::{
    rle::{CanRemove, HasLength, Sliceable},
    BTree, BTreeTrait, Cursor, FindResult, LeafIndex, Query, SplittedLeaves,
};
use itertools::Itertools;
use loro_common::{Counter, HasCounter, HasCounterSpan, HasIdSpan, IdSpan, Lamport, ID};
use smallvec::{smallvec, SmallVec};

use crate::container::richtext::{fugue_span::DiffStatus, FugueSpan, RichtextChunk, Status};

use super::UNKNOWN_PEER_ID;

#[derive(Debug, Default, Clone)]
pub(super) struct CrdtRope {
    pub(super) tree: BTree<CrdtRopeTrait>,
}

pub(super) struct InsertResult {
    #[allow(unused)]
    pub content: FugueSpan,
    pub leaf: LeafIndex,
    pub splitted: SplittedLeaves,
}

impl CrdtRope {
    pub fn new() -> Self {
        Self { tree: BTree::new() }
    }

    #[inline(always)]
    #[allow(unused)]
    pub fn len(&self) -> usize {
        self.tree.root_cache().len as usize
    }

    #[inline(always)]
    pub(super) fn tree(&self) -> &BTree<CrdtRopeTrait> {
        &self.tree
    }

    pub(super) fn insert(
        &mut self,
        pos: usize,
        mut content: FugueSpan,
        find_elem: impl Fn(ID) -> LeafIndex,
    ) -> InsertResult {
        if self.tree.is_empty() {
            assert_eq!(pos, 0);
            let leaf = self.tree.push(content).leaf;
            return InsertResult {
                content,
                leaf,
                splitted: SplittedLeaves::default(),
            };
        }

        // tracing::span!(tracing::Level::INFO, "Inserting {} len={}", content.id, content.rle_len());

        let pos = pos as i32;
        let start = self.tree.query::<ActiveLenQueryPreferLeft>(&pos).unwrap();

        let (parent_right_leaf, in_between) = {
            // calculate origin_left and origin_right
            // origin_left is the alive op at `pos-1`, origin_right is the first non-future op between `pos-1` and `pos`.

            // `start` may point to a zero len node that's before the active index.
            let origin_left = if start.cursor.offset == 0 {
                // get left leaf node if offset == 0, so we can calculate the origin_left
                if let Some(left) = self.tree.prev_elem(start.cursor) {
                    let left_node = self.tree.get_leaf(left.leaf.into());
                    assert!(left_node.elem().rle_len() > 0);
                    Some(
                        left_node
                            .elem()
                            .id
                            .inc(left_node.elem().rle_len() as Counter - 1)
                            .id(),
                    )
                } else {
                    None
                }
            } else {
                let left_node = self.tree.get_leaf(start.leaf().into());
                assert!(left_node.elem().rle_len() >= start.offset());
                Some(left_node.elem().id.inc(start.offset() as Counter - 1).id())
            };

            let (origin_right, parent_right_leaf, in_between) = {
                let mut in_between = Vec::new();
                let mut origin_right = None;
                let mut parent_right_idx = None;
                for iter in self.tree.iter_range(start.cursor..) {
                    if let Some(offset) = iter.start {
                        if offset >= iter.elem.rle_len() {
                            continue;
                        }
                    }

                    if !iter.elem.status.future {
                        origin_right =
                            Some(iter.elem.id.inc(iter.start.unwrap_or(0) as Counter).id());
                        let parent_right = match iter.start {
                            Some(offset) if offset > 0 => {
                                // It's guaranteed that origin_right's origin_left == this.origin_left.
                                // Because the first non-future node is just the leaf node in `start.cursor` node (iter.start.is_some())
                                // Thus parent_right = origin_right
                                Some(origin_right)
                            }
                            _ => {
                                // Otherwise, we need to test whether origin_right's origin_left == this.origin_left
                                if iter.elem.origin_left.map(|x| x.to_id()) == origin_left {
                                    Some(origin_right)
                                } else {
                                    None
                                }
                            }
                        };
                        parent_right_idx = parent_right.map(|_| iter.cursor().leaf);
                        break;
                    }

                    // elem must be from future
                    in_between.push((iter.cursor().leaf, *iter.elem));
                }

                (origin_right, parent_right_idx, in_between)
            };

            content.origin_left = origin_left.map(|x| x.try_into().unwrap());
            content.origin_right = origin_right.map(|x| x.try_into().unwrap());
            (parent_right_leaf, in_between)
        };

        let mut insert_pos = start.cursor;

        if !in_between.is_empty() {
            // find insert pos
            let mut scanning = false;
            let mut visited: SmallVec<[IdSpan; 4]> = Default::default();
            for (other_leaf, other_elem) in in_between.iter() {
                // tracing::info!("Visiting {}", &other_elem.id);
                let other_origin_left = other_elem.origin_left;
                if other_origin_left != content.origin_left
                    && other_origin_left
                        .map(|left| visited.iter().all(|x| !x.contains_id(left.to_id())))
                        .unwrap_or(true)
                {
                    // The other_elem's origin_left must be at the left side of content's origin_left.
                    // So the content must be at the left side of other_elem.

                    // tracing::info!("Break because the node's origin_left is at the left side of new_elem's origin left");
                    break;
                }

                visited.push(IdSpan::new(
                    other_elem.id.peer,
                    other_elem.id.counter,
                    other_elem.id.counter + other_elem.rle_len() as Counter,
                ));

                if content.origin_left == other_origin_left {
                    if other_elem.origin_right == content.origin_right {
                        // tracing::info!("Same right parent");
                        // Same right parent
                        if other_elem.id.peer > content.id.peer {
                            // tracing::info!("Break on larger peer");
                            break;
                        } else {
                            scanning = false;
                        }
                    } else {
                        // tracing::info!("Different right parent");
                        // Different right parent, we need to compare the right parents' position

                        let other_parent_right_idx =
                            if let Some(other_origin_right) = other_elem.origin_right {
                                let elem_idx = find_elem(other_origin_right.to_id());
                                let elem = self.tree.get_elem(elem_idx).unwrap();
                                // It must be the start of the elem
                                assert_eq!(elem.id.id(), other_origin_right.to_id());
                                if elem.origin_left == content.origin_left {
                                    Some(elem_idx)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                        match self.cmp_pos(other_parent_right_idx, parent_right_leaf) {
                            Ordering::Less => {
                                // tracing::info!("Less");
                                scanning = true;
                            }
                            Ordering::Equal if other_elem.id.peer > content.id.peer => {
                                // tracing::info!("Break on eq");
                                break;
                            }
                            _ => {
                                // tracing::info!("Scanning");
                                scanning = false;
                            }
                        }
                    }
                }

                if !scanning {
                    insert_pos = Cursor {
                        leaf: *other_leaf,
                        offset: other_elem.rle_len(),
                    };
                    // tracing::info!("updating insert pos {:?}", &insert_pos);
                }
            }
        }

        // tracing::info!("Inserting at {:?}", insert_pos);
        //
        let (cursor, splitted) = self.tree.insert_by_path(insert_pos, content);
        InsertResult {
            content,
            leaf: cursor.leaf,
            splitted,
        }
    }

    /// Delete the span at `pos` with `len`.
    ///
    /// The `start_id` is used to mark the real_id in the [FugueSpan].
    ///
    /// - If reversed is true, the deletion will be done in reversed order.
    ///   But the start_id always refers to the first delete op's id.
    /// - If reversed is true, the returned `SplittedLeaves` will be in reversed order.
    pub(super) fn delete(
        &mut self,
        mut start_id: ID,
        pos: usize,
        len: usize,
        reversed: bool,
        notify_deleted_span: &mut dyn FnMut(&FugueSpan),
    ) -> SmallVec<[SplittedLeaves; 1]> {
        if len == 0 {
            return Default::default();
        }

        if reversed && len > 1 {
            // TODO: PERF: optimize this
            let mut ans = SmallVec::with_capacity(len);
            for i in (0..len).rev() {
                let a = self.delete(
                    start_id.inc(i as i32),
                    pos + i,
                    1,
                    false,
                    notify_deleted_span,
                );

                ans.extend(a);
            }

            return ans;
        }

        let start = self
            .tree
            .query::<ActiveLenQueryPreferRight>(&(pos as i32))
            .unwrap();
        // avoid pointing to the end of the node
        let start = start.cursor;
        let elem = self.tree.get_elem_mut(start.leaf).unwrap();
        if elem.rle_len() >= start.offset + len {
            // tracing::info!("len={} offset={} l={} ", elem.rle_len(), start.offset, len,);
            let (_, splitted) = self.tree.update_leaf(start.leaf, |elem| {
                let (a, b) = elem.update_with_split(start.offset..start.offset + len, |elem| {
                    assert!(elem.is_activated());
                    debug_assert_eq!(len, elem.rle_len());
                    notify_deleted_span(elem);
                    elem.status.delete_times += 1;
                    if elem.real_id.is_none() {
                        elem.real_id = Some(start_id.try_into().unwrap());
                    }

                    start_id = start_id.inc(elem.rle_len() as i32);
                });

                (true, a, b)
            });

            return smallvec![splitted];
        }

        let end = self
            .tree
            .query::<ActiveLenQueryPreferLeft>(&((pos + len) as i32))
            .unwrap();
        smallvec![self.tree.update(start..end.cursor(), &mut |elem| {
            if elem.is_activated() {
                notify_deleted_span(elem);
                elem.status.delete_times += 1;
                if elem.real_id.is_none() {
                    elem.real_id = Some(start_id.try_into().unwrap());
                }

                start_id = start_id.inc(elem.rle_len() as i32);
                Some(Cache {
                    len: -(elem.rle_len() as i32),
                    changed_num: 0,
                })
            } else {
                None
            }
        })]
    }

    #[allow(unused)]
    pub(crate) fn diagnose(&self) {
        println!("crdt_rope number of tree nodes = {}", self.tree.node_len());
    }

    /// Update the leaf with given `id_span`
    ///
    /// Return the new leaf indexes that are created by splitting the old leaf nodes
    pub(super) fn update(
        &mut self,
        mut updates: Vec<LeafUpdate>,
        on_diff_status: bool,
    ) -> Vec<LeafIndex> {
        updates.sort_by_key(|x| x.leaf);
        let mut tree_update_info = Vec::with_capacity(updates.len());
        for (leaf, group) in &updates.into_iter().group_by(|x| x.leaf) {
            let elem = self.tree.get_elem(leaf).unwrap();
            for u in group {
                debug_assert_eq!(u.id_span.peer, elem.id.peer);
                let start = (u.id_span.ctr_start() - elem.id.counter).max(0);
                let end = (u.id_span.ctr_end() - elem.id.counter).max(0);
                let len = elem.rle_len();

                tree_update_info.push((leaf, (start as usize).min(len)..(end as usize).min(len), u))
            }
        }

        self.tree
            .update_leaves_with_arg_in_ranges(tree_update_info, |elem, arg| {
                let status = if on_diff_status {
                    if elem.diff_status.is_none() {
                        elem.diff_status = Some(elem.status);
                    }
                    elem.diff_status.as_mut().unwrap()
                } else {
                    &mut elem.status
                };

                if let Some(f) = arg.set_future {
                    status.future = f;
                }

                status.delete_times += arg.delete_times_diff;
            })
    }

    pub(super) fn clear_diff_status(&mut self) {
        self.tree.update_cache_and_elem_with_filter(
            |cache| {
                let drill = cache.changed_num > 0;
                cache.changed_num = 0;
                drill
            },
            |elem| {
                elem.diff_status = None;
            },
        );
    }

    pub(super) fn get_diff(&self) -> impl Iterator<Item = CrdtRopeDelta> + '_ {
        let mut last_pos = 0;
        let mut iter = self
            .tree
            .iter_with_filter(|cache| (cache.changed_num > 0, cache.len));
        let mut next = None;
        std::iter::from_fn(move || {
            if let Some(next) = next.take() {
                return Some(next);
            }

            #[allow(clippy::while_let_on_iterator)]
            while let Some((index, elem)) = iter.next() {
                // The elements will not be changed by this method.
                // This index is current index of the elem (calculated by `status` field rather than `diff_status` field)
                match elem.diff() {
                    DiffStatus::NotChanged => {}
                    DiffStatus::Created => {
                        let rt = Some(CrdtRopeDelta::Insert {
                            chunk: elem.content,
                            id: elem.real_id.map(|x| x.to_id()).unwrap(),
                            lamport: if elem.id.peer == UNKNOWN_PEER_ID {
                                None
                            } else {
                                Some(elem.id.lamport)
                            },
                        });
                        if index > last_pos {
                            next = rt;
                            let len = index - last_pos;
                            // last pos = index, because the creation has not been applied to the elem
                            last_pos = index;
                            return Some(CrdtRopeDelta::Retain(len as usize));
                        } else {
                            return rt;
                        }
                    }
                    DiffStatus::Deleted => {
                        let rt = Some(CrdtRopeDelta::Delete(elem.rle_len()));
                        if index > last_pos {
                            next = rt;
                            let len = index - last_pos;
                            // last pos = index + len, because the deletion has not been applied to the elem
                            last_pos = index + elem.rle_len() as i32;
                            return Some(CrdtRopeDelta::Retain(len as usize));
                        } else {
                            last_pos = index + elem.rle_len() as i32;
                            return rt;
                        }
                    }
                }
            }

            None
        })
    }

    fn cmp_pos(
        &self,
        parent_right: Option<LeafIndex>,
        other_parent_right: Option<LeafIndex>,
    ) -> Ordering {
        match (parent_right, other_parent_right) {
            (Some(a), Some(b)) => self
                .tree
                .compare_pos(Cursor { leaf: a, offset: 0 }, Cursor { leaf: b, offset: 0 }),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum CrdtRopeDelta {
    Retain(usize),
    Insert {
        chunk: RichtextChunk,
        id: ID,
        /// This is a optional field, because we may not know the correct lamport
        /// for chunk id with UNKNOWN_PEER_ID.
        ///
        /// This case happens when the chunk is created by default placeholder and
        /// the deletion happens that marks the chunk with its start_id. But it doesn't
        /// know the correct lamport for the chunk.
        lamport: Option<Lamport>,
    },
    Delete(usize),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CrdtRopeTrait;

#[derive(Debug, Default, Clone, PartialEq, Eq, Copy)]
pub(super) struct Cache {
    pub(super) len: i32,
    pub(super) changed_num: i32,
}

impl CanRemove for Cache {
    fn can_remove(&self) -> bool {
        self.len == 0 && self.changed_num == 0
    }
}

impl BTreeTrait for CrdtRopeTrait {
    type Elem = FugueSpan;
    type Cache = Cache;
    type CacheDiff = Cache;
    const USE_DIFF: bool = true;

    #[inline(always)]
    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> Self::CacheDiff {
        let new_len = caches.iter().map(|x| x.cache.len).sum();
        let new_changed_num = caches.iter().map(|x| x.cache.changed_num).sum();
        let len_diff = new_len - cache.len;
        let changed_num_diff = new_changed_num - cache.changed_num;
        cache.len = new_len;
        cache.changed_num = new_changed_num;
        Cache {
            len: len_diff,
            changed_num: changed_num_diff,
        }
    }

    #[inline(always)]
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        cache.len += diff.len;
        cache.changed_num += diff.changed_num;
    }

    #[inline(always)]
    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        diff1.len += diff2.len;
        diff1.changed_num += diff2.changed_num;
    }

    #[inline(always)]
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        Cache {
            len: elem.activated_len() as i32,
            changed_num: if elem.diff_status.is_some() { 1 } else { 0 },
        }
    }

    #[inline(always)]
    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }

    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
        Cache {
            len: cache_lhs.len - cache_rhs.len,
            changed_num: cache_lhs.changed_num - cache_rhs.changed_num,
        }
    }
}

/// Query for start position, prefer left.
///
/// If there are zero length spans (deleted, or spans from future) at the
/// active index, the query will return the position of the first non-zero length
/// content before them.
///
/// NOTE: it may points to the end of a leaf node (with offset = rle_len) while the next leaf node is available.
struct ActiveLenQueryPreferLeft {
    left: i32,
}

impl Query<CrdtRopeTrait> for ActiveLenQueryPreferLeft {
    type QueryArg = i32;

    fn init(target: &Self::QueryArg) -> Self {
        debug_assert!(*target >= 0);
        Self { left: *target }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<CrdtRopeTrait>],
    ) -> generic_btree::FindResult {
        let mut left = self.left;
        for (i, child) in child_caches.iter().enumerate() {
            let cache = &child.cache;
            if left <= cache.len {
                // Prefer left. So if both `cache.len` and `left` equal zero, return here.
                self.left = left;
                return generic_btree::FindResult::new_found(i, left as usize);
            }

            left -= cache.len;
        }

        if let Some(last) = child_caches.last() {
            left += last.cache.len;
            self.left = left;
            FindResult::new_missing(child_caches.len() - 1, left as usize)
        } else {
            // TODO: this should be impossible
            unreachable!()
        }
    }

    fn confirm_elem(
        &mut self,
        _: &Self::QueryArg,
        elem: &<CrdtRopeTrait as BTreeTrait>::Elem,
    ) -> (usize, bool) {
        if elem.is_activated() {
            (self.left as usize, (self.left as usize) < elem.rle_len())
        } else {
            // prefer left on zero length spans
            (0, self.left == 0)
        }
    }
}

/// Query for start position, prefer right
///
/// If there are zero length spans (deleted, or spans from future) at the
/// active index, the query will return the position of the first non-zero length
/// content after them.
struct ActiveLenQueryPreferRight {
    left: i32,
}

impl Query<CrdtRopeTrait> for ActiveLenQueryPreferRight {
    type QueryArg = i32;

    fn init(target: &Self::QueryArg) -> Self {
        debug_assert!(*target >= 0);
        Self { left: *target }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<CrdtRopeTrait>],
    ) -> generic_btree::FindResult {
        let mut left = self.left;
        for (i, child) in child_caches.iter().enumerate() {
            let cache = &child.cache;
            if left < cache.len {
                // Prefer left. So if both `cache.len` and `left` equal zero, return here.
                self.left = left;
                return generic_btree::FindResult::new_found(i, left as usize);
            }

            left -= cache.len;
        }

        if let Some(last) = child_caches.last() {
            left += last.cache.len;
            self.left = left;
            FindResult::new_missing(child_caches.len() - 1, left as usize)
        } else {
            // TODO: this should be impossible
            unreachable!()
        }
    }

    fn confirm_elem(
        &mut self,
        _: &Self::QueryArg,
        elem: &<CrdtRopeTrait as BTreeTrait>::Elem,
    ) -> (usize, bool) {
        if elem.is_activated() {
            (self.left as usize, (self.left as usize) < elem.rle_len())
        } else {
            (self.left as usize, self.left == 0)
        }
    }
}

/// This struct describe the ways to update a leaf's status
#[derive(Clone, Debug, Copy)]
pub(super) struct LeafUpdate {
    pub leaf: LeafIndex,
    /// `id_span` should only contains a subset of the leaf content
    pub id_span: IdSpan,
    /// if `set_future` is `None`, the `future` field will not be changed
    pub set_future: Option<bool>,
    pub delete_times_diff: i16,
}

impl LeafUpdate {
    #[allow(unused)]
    fn apply_to(&self, s: &mut Status) {
        s.delete_times += self.delete_times_diff;
        if let Some(f) = self.set_future {
            s.future = f;
        }
    }
}

#[cfg(test)]
mod test {
    use std::ops::Range;

    use loro_common::{CompactId, Counter, IdFull, PeerID, ID};

    use crate::container::richtext::RichtextChunk;

    use super::*;

    fn span(id: u32, range: Range<u32>) -> FugueSpan {
        FugueSpan::new(
            IdFull::new(id as PeerID, 0 as Counter, 0),
            RichtextChunk::new_text(range),
        )
    }

    #[allow(unused)]
    fn unknown_span(id: u32, len: usize) -> FugueSpan {
        FugueSpan::new(
            IdFull::new(id as PeerID, 0 as Counter, 0),
            RichtextChunk::new_unknown(len as u32),
        )
    }

    fn future_span(id: u32, range: Range<u32>) -> FugueSpan {
        let mut fugue = FugueSpan::new(
            IdFull::new(id as PeerID, 0 as Counter, 0),
            RichtextChunk::new_text(range),
        );

        fugue.status.future = true;
        fugue
    }

    fn dead_span(id: u32, range: Range<u32>) -> FugueSpan {
        let mut span = FugueSpan::new(
            IdFull::new(id as PeerID, 0 as Counter, 0),
            RichtextChunk::new_text(range),
        );

        span.status.delete_times += 1;
        span
    }

    #[test]
    fn len_test() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10), |_| panic!());
        assert_eq!(rope.len(), 10);
        rope.insert(5, span(1, 0..10), |_| panic!());
        assert_eq!(rope.len(), 20);
        rope.insert(20, span(1, 0..10), |_| panic!());
        assert_eq!(rope.len(), 30);
        for i in 3..30 {
            assert_eq!(rope.len(), i * 10);
            rope.insert(
                i,
                span(i as u32, i as u32 * 10..(i as u32 + 1) * 10),
                |_| panic!(),
            );
        }
    }

    #[test]
    fn content_insert_middle() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10), |_| panic!());
        rope.insert(5, span(1, 10..20), |_| panic!());
        let arr: Vec<_> = rope.tree.iter().collect();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].rle_len(), 5);
        assert_eq!(arr[1].rle_len(), 10);
        assert_eq!(arr[2].rle_len(), 5);

        assert_eq!(arr[0].id.counter, 0);
        assert_eq!(arr[0].id.peer, 0);
        assert_eq!(arr[1].id.counter, 0);
        assert_eq!(arr[1].id.peer, 1);
        assert_eq!(arr[2].id.counter, 5);
        assert_eq!(arr[2].id.peer, 0);
    }

    #[test]
    fn content_insert_should_ignore_tombstone() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10), |_| panic!());
        // 0..10

        rope.insert(5, dead_span(1, 10..20), |_| panic!());
        // 0..5, 10..20(dead), 5..10

        rope.insert(10, span(0, 20..30), |_| panic!());
        // 0..5, 10..20(dead), 5..10, 20..30

        let arr: Vec<_> = rope.tree.iter().collect();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0].rle_len(), 5);
        assert_eq!(arr[1].rle_len(), 10);
        assert_eq!(arr[1].activated_len(), 0);
        assert_eq!(arr[2].rle_len(), 5);
        assert_eq!(arr[3].rle_len(), 10);
    }

    #[test]
    fn get_origin_left_and_right() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10), |_| panic!());
        let fugue = rope.insert(5, span(1, 10..20), |_| panic!()).content;
        assert_eq!(fugue.origin_left, Some(CompactId::new(0, 4)));
        assert_eq!(fugue.origin_right, Some(CompactId::new(0, 5)));
    }

    #[test]
    fn get_origin_left_and_right_among_tombstones() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10), |_| panic!());
        assert_eq!(rope.len(), 10);
        rope.delete(ID::NONE_ID, 5, 2, false, &mut |_| {});
        assert_eq!(rope.len(), 8);
        let fugue = rope.insert(6, span(1, 10..20), |_| panic!()).content;
        assert_eq!(fugue.origin_left, Some(CompactId::new(0, 7)));
        assert_eq!(fugue.origin_right, Some(CompactId::new(0, 8)));
        let fugue = rope.insert(5, span(1, 10..11), |_| panic!()).content;
        assert_eq!(fugue.origin_left, Some(CompactId::new(0, 4)));
        assert_eq!(fugue.origin_right, Some(CompactId::new(0, 5)));
    }

    #[test]
    fn should_ignore_future_spans_when_getting_origin_left() {
        {
            // insert future
            let mut rope = CrdtRope::new();
            rope.insert(0, span(0, 0..10), |_| panic!());
            rope.insert(5, future_span(1, 10..20), |_| panic!());
            let fugue = rope.insert(5, span(1, 10..20), |_| panic!()).content;
            assert_eq!(fugue.origin_left, Some(CompactId::new(0, 4)));
            assert_eq!(fugue.origin_right, Some(CompactId::new(0, 5)));
        }
        {
            // insert deleted
            let mut rope = CrdtRope::new();
            rope.insert(0, span(0, 0..10), |_| panic!());
            rope.insert(5, dead_span(1, 10..20), |_| panic!());
            let fugue = rope.insert(5, span(1, 10..20), |_| panic!()).content;
            assert_eq!(fugue.origin_left, Some(CompactId::new(0, 4)));
            assert_eq!(fugue.origin_right, Some(CompactId::new(1, 0)));
        }
    }

    #[test]
    fn update() {
        let mut rope = CrdtRope::new();
        let result = rope.insert(0, span(0, 0..10), |_| panic!());
        let split = rope.update(
            vec![LeafUpdate {
                leaf: result.leaf,
                id_span: IdSpan::new(0, 2, 8),
                set_future: None,
                delete_times_diff: 1,
            }],
            false,
        );

        assert_eq!(rope.len(), 4);
        assert_eq!(split.len(), 2);
        let split = rope.update(
            vec![LeafUpdate {
                leaf: split[0],
                id_span: IdSpan::new(0, 2, 8),
                set_future: None,
                delete_times_diff: -1,
            }],
            false,
        );

        assert_eq!(rope.len(), 10);
        assert_eq!(split.len(), 0);
    }

    #[test]
    fn checkout() {
        let mut rope = CrdtRope::new();
        let result1 = rope.insert(0, span(0, 0..10), |_| panic!());
        let result2 = rope.insert(10, dead_span(1, 10..20), |_| panic!());
        rope.update(
            vec![
                LeafUpdate {
                    leaf: result1.leaf,
                    id_span: IdSpan::new(0, 2, 8),
                    set_future: None,
                    delete_times_diff: 1,
                },
                LeafUpdate {
                    leaf: result2.leaf,
                    id_span: IdSpan::new(1, 0, 3),
                    set_future: None,
                    delete_times_diff: -1,
                },
            ],
            true,
        );
        let vec: Vec<_> = rope.get_diff().collect();
        assert_eq!(
            vec![
                CrdtRopeDelta::Retain(2),
                CrdtRopeDelta::Delete(6),
                CrdtRopeDelta::Retain(2),
                CrdtRopeDelta::Insert {
                    chunk: RichtextChunk::new_text(10..13),
                    id: ID::new(1, 0),
                    lamport: Some(0)
                }
            ],
            vec,
        );
    }

    #[test]
    fn checkout_future() {
        let mut rope = CrdtRope::new();
        let result = rope.insert(0, future_span(0, 0..10), |_| panic!());
        rope.update(
            vec![LeafUpdate {
                leaf: result.leaf,
                id_span: IdSpan::new(0, 2, 10),
                set_future: Some(false),
                delete_times_diff: 0,
            }],
            true,
        );
        let vec: Vec<_> = rope.get_diff().collect();
        assert_eq!(
            vec![CrdtRopeDelta::Insert {
                chunk: RichtextChunk::new_text(2..10),
                id: ID::new(0, 2),
                lamport: Some(2)
            }],
            vec,
        );
    }

    #[test]
    fn checkout_future_with_delete() {
        let mut rope = CrdtRope::new();
        let result = rope.insert(0, future_span(0, 0..10), |_| panic!());
        rope.update(
            vec![LeafUpdate {
                leaf: result.leaf,
                id_span: IdSpan::new(0, 2, 10),
                set_future: Some(false),
                delete_times_diff: 1,
            }],
            true,
        );
        let vec: Vec<_> = rope.get_diff().collect();
        assert!(vec.is_empty());
    }
}
