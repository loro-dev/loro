use std::cmp::Ordering;

use super::fugue_span::{FugueSpan, Status};
use fxhash::FxHashSet;
use generic_btree::{
    rle::{HasLength, Sliceable},
    BTree, BTreeTrait, Cursor, FindResult, LeafIndex, Query, SplittedLeaves,
};
use loro_common::{Counter, HasCounter, HasCounterSpan, HasIdSpan, IdSpan, ID};
use smallvec::SmallVec;

#[derive(Debug, Default, Clone)]
pub(super) struct CrdtRope {
    tree: BTree<CrdtRopeTrait>,
}

pub(super) struct InsertResult {
    pub content: FugueSpan,
    pub leaf: LeafIndex,
    pub splitted: SplittedLeaves,
}

impl CrdtRope {
    pub fn new() -> Self {
        Self { tree: BTree::new() }
    }

    #[inline(always)]
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
            let leaf = self.tree.push(content);
            return InsertResult {
                content,
                leaf,
                splitted: SplittedLeaves::default(),
            };
        }

        let pos = pos as isize;
        let start = self.tree.query::<ActiveLenQuery>(&pos).unwrap();

        let (parent_right, parent_right_leaf, in_between) = {
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
                            .inc(left_node.elem().rle_len() as Counter - 1),
                    )
                } else {
                    None
                }
            } else {
                let left_node = self.tree.get_leaf(start.leaf().into());
                assert!(left_node.elem().rle_len() >= start.offset());
                Some(left_node.elem().id.inc(start.offset() as Counter - 1))
            };

            let (origin_right, parent_right, parent_right_leaf, in_between) = if pos
                == self.tree.root_cache().len
            {
                (None, None, None, Vec::new())
            } else {
                let mut in_between = Vec::new();
                let mut origin_right = None;
                let mut parent_right = None;
                let mut parent_right_idx = None;
                for iter in self.tree.iter_range(start.cursor..) {
                    if !iter.elem.status.future {
                        origin_right = Some(iter.elem.id.inc(iter.start.unwrap_or(0) as Counter));
                        parent_right = match iter.start {
                            Some(_) => {
                                // It's guaranted that origin_right's origin_left == this.origin_left.
                                // Because the first non-future node is just the leaf node in `start.cursor` node (iter.start.is_some())
                                // Thus parent_right = origin_right
                                Some(origin_right)
                            }
                            _ => {
                                // Otherwise, we need to test whether origin_right's origin_left == this.origin_left
                                if iter.elem.origin_left == origin_left {
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

                (origin_right, parent_right, parent_right_idx, in_between)
            };

            content.origin_left = origin_left;
            content.origin_right = origin_right;
            (parent_right, parent_right_leaf, in_between)
        };

        let mut insert_pos = start.cursor;

        if !in_between.is_empty() {
            // find insert pos
            let mut scanning = false;
            let mut visited: SmallVec<[IdSpan; 4]> = Default::default();
            for (i, (other_leaf, other_elem)) in in_between.iter().enumerate() {
                let other_orign_left = other_elem.origin_left;
                if other_orign_left
                    .map(|left| visited.iter().all(|x| !x.contains_id(left)))
                    .unwrap_or(true)
                    && other_orign_left != content.origin_left
                {
                    // The other_elem's origin_left must be at the left side of content's origin_left.
                    // So the content must be at the left side of other_elem.
                    break;
                }

                visited.push(IdSpan::new(
                    other_elem.id.peer,
                    other_elem.id.counter,
                    other_elem.id.counter + other_elem.rle_len() as Counter,
                ));

                if content.origin_left == other_orign_left {
                    if other_elem.origin_right == content.origin_right {
                        // Same right parent
                        if other_elem.id.peer > content.id.peer {
                            break;
                        } else {
                            scanning = false;
                        }
                    } else {
                        // Different right parent, we need to compare the right parents' position

                        let other_parent_right_idx =
                            if let Some(other_origin_right) = other_elem.origin_right {
                                let elem_idx = find_elem(other_origin_right);
                                let elem = self.tree.get_elem(elem_idx).unwrap();
                                // It must be the start of the elem
                                assert_eq!(elem.id, other_origin_right);
                                if elem.origin_left == content.origin_left {
                                    Some(elem_idx)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                        match self.cmp_pos(parent_right_leaf, other_parent_right_idx) {
                            Ordering::Less => {
                                scanning = true;
                            }
                            Ordering::Equal if content.id.peer > other_elem.id.peer => {
                                break;
                            }
                            _ => {
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
                }
            }
        }

        let (leaf, splitted) = self.tree.insert_by_path(insert_pos, content);
        InsertResult {
            content,
            leaf,
            splitted,
        }
    }

    pub(super) fn delete(
        &mut self,
        pos: usize,
        len: usize,
        mut notify_deleted_span: impl FnMut(FugueSpan),
    ) -> SplittedLeaves {
        let start = self.tree.query::<ActiveLenQuery>(&(pos as isize)).unwrap();
        let end = self
            .tree
            .query::<ActiveLenQuery>(&((pos + len) as isize))
            .unwrap();
        self.tree.update(start.cursor()..end.cursor(), &mut |elem| {
            if elem.is_activated() {
                notify_deleted_span(*elem);
                elem.status.delete_times += 1;
                Some(Cache {
                    len: -(elem.rle_len() as isize),
                })
            } else {
                None
            }
        })
    }

    /// Update the leaf with given `id_span`
    ///
    /// Return the new leaf indexes that are created by splitting the old leaf nodes
    pub(super) fn update(&mut self, updates: &[LeafUpdate]) -> Vec<LeafIndex> {
        let mut ans = Vec::new();
        // TODO: this method can be optimized by batching the updates
        for update in updates {
            let (_, splitted) = self.tree.update_leaf(update.leaf, |elem| {
                let start = update.id_span.ctr_start() - elem.id.counter;
                let end = update.id_span.ctr_end() - elem.id.counter;
                let mut diff = 0;
                let (a, b) = elem.update_with_split(start as usize..end as usize, |elem| {
                    let was_active = elem.is_activated();
                    update.apply_to(&mut elem.status);
                    match (was_active, elem.is_activated()) {
                        (true, false) => {
                            diff -= elem.rle_len() as isize;
                        }
                        (false, true) => {
                            diff += elem.rle_len() as isize;
                        }
                        _ => {}
                    }
                });

                (true, Some(Cache { len: diff }), a, b)
            });

            for s in splitted.arr {
                ans.push(s);
            }
        }

        ans
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

#[derive(Debug, Clone, Copy)]
pub(super) struct CrdtRopeTrait;

#[derive(Debug, Default, Clone, PartialEq, Eq, Copy)]
pub(super) struct Cache {
    pub(super) len: isize,
    // TODO: consider adding a 'changed_num' field
}

impl BTreeTrait for CrdtRopeTrait {
    type Elem = FugueSpan;

    type Cache = Cache;

    type CacheDiff = Cache;

    #[inline(always)]
    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> Self::CacheDiff {
        let ans = caches.iter().map(|x| x.cache.len).sum();
        let diff = ans - cache.len;
        cache.len = ans;
        Cache { len: diff }
    }

    #[inline(always)]
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        cache.len += diff.len;
    }

    #[inline(always)]
    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        diff1.len += diff2.len;
    }

    #[inline(always)]
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        Cache {
            len: elem.activated_len() as isize,
        }
    }

    #[inline(always)]
    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }
}

/// Query for start position, prefer left.
///
/// If there are zero length spans (deleted, or spans from future) before the
/// active index, the query will return the position of the first non-zero length
/// content.
struct ActiveLenQuery {
    left: isize,
}

impl Query<CrdtRopeTrait> for ActiveLenQuery {
    type QueryArg = isize;

    fn init(target: &Self::QueryArg) -> Self {
        debug_assert!(*target >= 0);
        ActiveLenQuery { left: *target }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<CrdtRopeTrait>],
    ) -> generic_btree::FindResult {
        let mut left = self.left;
        for (i, child) in child_caches.iter().enumerate() {
            let cache = &child.cache;
            if (cache.len == 0 && left == 0) || left < cache.len {
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
            // TODO: this should be imppssible
            unreachable!()
        }
    }

    fn confirm_elem(
        &self,
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

    use loro_common::{Counter, PeerID, ID};

    use crate::container::richtext::tracker::fugue_span::Content;

    use super::*;

    fn span(id: u32, range: Range<u32>) -> FugueSpan {
        FugueSpan::new(
            ID::new(id as PeerID, 0 as Counter),
            Content::new_text(range),
        )
    }

    fn unknown_span(id: u32, len: usize) -> FugueSpan {
        FugueSpan::new(
            ID::new(id as PeerID, 0 as Counter),
            Content::new_unknown(len as u32),
        )
    }

    fn future_span(id: u32, range: Range<u32>) -> FugueSpan {
        let mut fugue = FugueSpan::new(
            ID::new(id as PeerID, 0 as Counter),
            Content::new_text(range),
        );

        fugue.status.future = true;
        fugue
    }

    fn dead_span(id: u32, range: Range<u32>) -> FugueSpan {
        let mut span = FugueSpan::new(
            ID::new(id as PeerID, 0 as Counter),
            Content::new_text(range),
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
        assert_eq!(fugue.origin_left, Some(ID::new(0, 4)));
        assert_eq!(fugue.origin_right, Some(ID::new(0, 5)));
    }

    #[test]
    fn get_origin_left_and_right_among_tombstones() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10), |_| panic!());
        assert_eq!(rope.len(), 10);
        rope.delete(5, 2, |_| {});
        assert_eq!(rope.len(), 8);
        let fugue = rope.insert(6, span(1, 10..20), |_| panic!()).content;
        assert_eq!(fugue.origin_left, Some(ID::new(0, 7)));
        assert_eq!(fugue.origin_right, Some(ID::new(0, 8)));
        let fugue = rope.insert(5, span(1, 10..11), |_| panic!()).content;
        assert_eq!(fugue.origin_left, Some(ID::new(0, 4)));
        assert_eq!(fugue.origin_right, Some(ID::new(0, 5)));
    }

    #[test]
    fn should_ignore_future_spans_when_getting_origin_left() {
        {
            // insert future
            let mut rope = CrdtRope::new();
            rope.insert(0, span(0, 0..10), |_| panic!());
            rope.insert(5, future_span(1, 10..20), |_| panic!());
            let fugue = rope.insert(5, span(1, 10..20), |_| panic!()).content;
            assert_eq!(fugue.origin_left, Some(ID::new(0, 4)));
            assert_eq!(fugue.origin_right, Some(ID::new(0, 5)));
        }
        {
            // insert deleted
            let mut rope = CrdtRope::new();
            rope.insert(0, span(0, 0..10), |_| panic!());
            rope.insert(5, dead_span(1, 10..20), |_| panic!());
            let fugue = rope.insert(5, span(1, 10..20), |_| panic!()).content;
            assert_eq!(fugue.origin_left, Some(ID::new(0, 4)));
            assert_eq!(fugue.origin_right, Some(ID::new(1, 0)));
        }
    }

    #[test]
    fn update() {
        let mut rope = CrdtRope::new();
        let result = rope.insert(0, span(0, 0..10), |_| panic!());
        let split = rope.update(&[LeafUpdate {
            leaf: result.leaf,
            id_span: IdSpan::new(0, 2, 8),
            set_future: None,
            delete_times_diff: 1,
        }]);

        assert_eq!(rope.len(), 4);
        assert_eq!(split.len(), 2);
        let split = rope.update(&[LeafUpdate {
            leaf: split[0],
            id_span: IdSpan::new(0, 2, 8),
            set_future: None,
            delete_times_diff: -1,
        }]);

        assert_eq!(rope.len(), 10);
        assert_eq!(split.len(), 0);
    }

    #[test]
    #[ignore]
    fn checkout() {
        todo!()
    }
}
