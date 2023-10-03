use super::fugue_span::FugueSpan;
use generic_btree::{
    rle::HasLength, BTree, BTreeTrait, FindResult, LeafIndex, Query, SplittedLeaves,
};
use loro_common::Counter;

#[derive(Debug, Clone)]
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

    pub(super) fn insert(&mut self, pos: usize, mut content: FugueSpan) -> InsertResult {
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

        {
            // calculate origin_left and origin_right

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

            let origin_right = if pos == self.tree.root_cache().len {
                None
            } else {
                let elem = self.tree.get_elem(start.cursor.leaf).unwrap();
                Some(elem.id.inc(start.offset() as Counter))
            };

            content.origin_left = origin_left;
            content.origin_right = origin_right;
        }

        let (leaf, splitted) = self.tree.insert_by_path(start.cursor, content);
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
            }

            None
        })
    }

    pub(super) fn apply(&mut self) {}
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
        rope.insert(0, span(0, 0..10));
        assert_eq!(rope.len(), 10);
        rope.insert(5, span(1, 0..10));
        assert_eq!(rope.len(), 20);
        rope.insert(20, span(1, 0..10));
        assert_eq!(rope.len(), 30);
        for i in 3..30 {
            assert_eq!(rope.len(), i * 10);
            rope.insert(i, span(i as u32, i as u32 * 10..(i as u32 + 1) * 10));
        }
    }

    #[test]
    fn content_insert_middle() {
        let mut rope = CrdtRope::new();
        rope.insert(0, span(0, 0..10));
        rope.insert(5, span(1, 10..20));
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
        rope.insert(0, span(0, 0..10));
        // 0..10

        rope.insert(5, dead_span(1, 10..20));
        // 0..5, 10..20(dead), 5..10

        rope.insert(10, span(0, 20..30));
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
        rope.insert(0, span(0, 0..10));
        let fugue = rope.insert(5, span(1, 10..20)).content;
        assert_eq!(fugue.origin_left, Some(ID::new(0, 4)));
        assert_eq!(fugue.origin_right, Some(ID::new(0, 5)));
    }

    #[test]
    #[ignore]
    fn should_ignore_deleted_spans_when_getting_origin_left() {
        todo!()
    }

    #[test]
    #[ignore]
    fn should_not_ignore_deleted_spans_when_getting_origin_right() {
        todo!()
    }

    #[test]
    #[ignore]
    fn should_ignore_future_spans_when_getting_origin_left() {
        todo!()
    }

    #[test]
    #[ignore]
    fn delete() {
        todo!()
    }

    #[test]
    #[ignore]
    fn delete_twice() {
        todo!()
    }
}
