extern crate alloc;

use core::ops::RangeBounds;
use std::assert_eq;
use std::fmt::Display;

use crate::generic_impl::gap_buffer::MAX_STRING_SIZE;
use crate::rle::Sliceable;
use crate::{BTree, BTreeTrait, LeafIndex, LengthFinder, QueryResult};

use super::gap_buffer::GapBuffer;
use super::len_finder::UseLengthFinder;

#[derive(Debug)]
struct RopeTrait;

#[derive(Debug)]
struct Cursor {
    pos: usize,
    leaf: LeafIndex,
}

// TODO: move Rope into a separate project
#[derive(Debug)]
pub struct Rope {
    tree: BTree<RopeTrait>,
    cursor: Option<Cursor>,
}

impl UseLengthFinder<RopeTrait> for RopeTrait {
    #[inline(always)]
    fn get_len(cache: &<Self as BTreeTrait>::Cache) -> usize {
        *cache as usize
    }
}

impl Rope {
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.tree.root_cache as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.tree.root_cache == 0
    }

    pub fn insert(&mut self, index: usize, elem: &str) {
        if index > self.len() {
            panic!("index {} out of range len={}", index, self.len());
        }

        if self.is_empty() {
            for chunk in GapBuffer::from_str(elem) {
                self.tree.push(chunk);
            }
            return;
        }

        if let Some(Cursor { pos, leaf }) = self.cursor {
            if pos <= index {
                let node = self.tree.leaf_nodes.get(leaf.0).unwrap();
                if index <= pos + node.elem.len() {
                    let mut success = true;
                    let offset = index - pos;
                    let valid = self
                        .tree
                        .update_leaf(leaf, |leaf| {
                            if leaf.len() + elem.len() < MAX_STRING_SIZE {
                                leaf.insert_bytes(offset, elem.as_bytes()).unwrap();
                                (true, None, None)
                            } else {
                                let mut right = leaf.split(offset);
                                if leaf.len() + elem.len() < MAX_STRING_SIZE {
                                    success = leaf.push_bytes(elem.as_bytes()).is_ok();
                                } else {
                                    success = right.insert_bytes(0, elem.as_bytes()).is_ok();
                                }

                                (true, Some(right), None)
                            }
                        })
                        .0;

                    if !valid {
                        self.cursor = None;
                    }

                    if success {
                        return;
                    }
                }
            }
        }

        let (q, f) = self.tree.query_with_finder_return::<LengthFinder>(&index);
        self.cursor = q.and_then(|q| {
            if q.offset() == 0 {
                if f.slot == 0 || f.parent.is_none() {
                    None
                } else {
                    let node = self.tree.in_nodes.get(f.parent.unwrap()).unwrap();
                    let child = &node.children[f.slot as usize - 1];
                    Some(Cursor {
                        pos: index - child.cache as usize,
                        leaf: child.arena.unwrap().into(),
                    })
                }
            } else {
                Some(Cursor {
                    pos: index - q.offset(),
                    leaf: q.leaf(),
                })
            }
        });

        self.tree
            .insert_many_by_cursor(q.map(|x| x.cursor), GapBuffer::from_str(elem));
    }

    pub fn delete_range(&mut self, range: impl RangeBounds<usize>) {
        if self.is_empty() {
            return;
        }

        let start = match range.start_bound() {
            core::ops::Bound::Included(x) => *x,
            core::ops::Bound::Excluded(x) => *x + 1,
            core::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            core::ops::Bound::Included(&x) => x + 1,
            core::ops::Bound::Excluded(&x) => x,
            core::ops::Bound::Unbounded => self.len(),
        };
        let end = end.min(self.len());
        let start = start.min(end);
        if start == end {
            return;
        }

        if let Some(Cursor { pos, leaf }) = self.cursor {
            if pos <= start {
                let node = self.tree.leaf_nodes.get(leaf.0).unwrap();
                if end <= pos + node.elem.len() {
                    let start_offset = start - pos;
                    let end_offset = end - pos;
                    let valid = self
                        .tree
                        .update_leaf(leaf, |leaf| {
                            leaf.delete(start_offset..end_offset);
                            (true, None, None)
                        })
                        .0;

                    if !valid {
                        self.cursor = None;
                    }

                    return;
                }
            }
        }

        if end - start == 1 {
            let q = self
                .tree
                .update_leaf_by_search::<LengthFinder>(&start, |leaf, pos| {
                    leaf.delete(pos.cursor.offset..pos.cursor.offset + 1);
                    Some((-1, None, None))
                });
            self.cursor = q.0.map(|q| Cursor {
                pos: start - q.offset,
                leaf: q.leaf,
            });

            return;
        }

        self.cursor = None;
        let from = self.tree.query::<LengthFinder>(&start);
        let to = self.tree.query::<LengthFinder>(&end);
        match (from, to) {
            (Some(from), Some(to)) if from.cursor.leaf == to.cursor.leaf => {
                let leaf = self.tree.leaf_nodes.get_mut(from.arena()).unwrap();
                if from.cursor.offset == 0 && to.cursor.offset == leaf.elem.len() {
                    // delete the whole leaf
                    self.tree.remove_leaf(from.cursor);
                } else {
                    leaf.elem.delete(from.cursor.offset..to.cursor.offset);
                    self.tree.recursive_update_cache(
                        from.leaf().into(),
                        true,
                        Some(start as isize - end as isize),
                    );
                }
            }
            _ => {
                crate::iter::Drain::new(&mut self.tree, from, to);
            }
        }
    }

    fn iter(&self) -> impl Iterator<Item = &GapBuffer> {
        let mut node_iter = self
            .tree
            .first_path()
            .map(|first| crate::iter::Iter::new(&self.tree, first, self.tree.last_path().unwrap()));
        std::iter::from_fn(move || match &mut node_iter {
            Some(node_iter) => {
                if let Some(node) = node_iter.next() {
                    Some(&node.1.elem)
                } else {
                    None
                }
            }
            None => None,
        })
    }

    pub fn slice(&mut self, _range: impl RangeBounds<usize>) {
        unimplemented!()
    }

    pub fn new() -> Self {
        Self {
            tree: BTree::new(),
            cursor: None,
        }
    }

    #[allow(unused)]
    fn node_len(&self) -> usize {
        self.tree.node_len()
    }

    #[allow(unused)]
    fn update_in_place(&mut self, pos: usize, new: &str) {
        todo!()
    }

    pub fn clear(&mut self) {
        self.tree.clear();
    }

    #[allow(unused)]
    pub fn check(&self) {
        // dbg!(&self.tree);
        self.tree.check()
    }

    pub fn diagnose(&self) {
        self.tree.diagnose_balance();
    }
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for Rope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ans = Vec::with_capacity(self.len());
        for elem in self.iter() {
            let (left, right) = elem.as_bytes();
            ans.extend_from_slice(left);
            ans.extend_from_slice(right);
        }

        f.write_str(std::str::from_utf8(ans.as_slice()).unwrap())
    }
}

impl BTreeTrait for RopeTrait {
    type Elem = GapBuffer;
    type Cache = isize;
    type CacheDiff = isize;

    #[inline(always)]
    fn calc_cache_internal(cache: &mut Self::Cache, caches: &[crate::Child<Self>]) -> isize {
        let new_cache = caches.iter().map(|x| x.cache).sum::<isize>();
        let diff = new_cache - *cache;
        *cache = new_cache;
        diff
    }

    #[inline(always)]
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        *cache += *diff;
    }

    #[inline(always)]
    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += diff2;
    }

    #[inline(always)]
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        elem.len() as isize
    }

    #[inline(always)]
    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }

    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
        cache_lhs - cache_rhs
    }
}

#[allow(unused)]
fn test_prev_length(rope: &Rope, q: QueryResult) -> usize {
    let mut count = 0;
    rope.tree
        .visit_previous_caches(q.cursor(), |cache| match cache {
            crate::PreviousCache::NodeCache(cache) => {
                count += *cache as usize;
            }
            crate::PreviousCache::PrevSiblingElem(p) => {
                count += p.len();
            }
            crate::PreviousCache::ThisElemAndOffset { offset, .. } => {
                count += offset;
            }
        });
    count
}

#[allow(unused)]
fn test_index(rope: &Rope) {
    for index in 0..rope.len() {
        let q = rope.tree.query::<LengthFinder>(&index).unwrap();
        let i = test_prev_length(rope, q);
        assert_eq!(i, index);
    }
}

#[cfg(test)]
mod test {

    use Action::*;

    use crate::HeapVec;

    use super::*;

    #[test]
    fn test() {
        let mut rope = Rope::new();
        rope.insert(0, "123");
        assert_eq!(rope.len(), 3);
        rope.insert(1, "x");
        test_index(&rope);
        assert_eq!(rope.len(), 4);
        rope.delete_range(2..4);
        assert_eq!(&rope.to_string(), "1x");
        rope.delete_range(..1);
        assert_eq!(&rope.to_string(), "x");
        rope.delete_range(..);
        assert_eq!(&rope.to_string(), "");
        assert_eq!(rope.len(), 0);
    }

    #[test]
    fn test_delete_middle() {
        let mut rope = Rope::new();
        rope.insert(0, "135");
        rope.delete_range(1..2);
        assert_eq!(&rope.to_string(), "15");
    }

    #[test]
    fn test_insert_repeatedly() {
        let mut rope = Rope::new();
        rope.insert(0, "123");
        rope.insert(1, "x");
        rope.insert(2, "y");
        rope.insert(3, "z");
        test_index(&rope);
        assert_eq!(&rope.to_string(), "1xyz23");
    }

    #[test]
    #[ignore]
    fn test_update() {
        let mut rope = Rope::new();
        rope.insert(0, "123");
        rope.insert(3, "xyz");
        rope.update_in_place(1, "kkkk");
        assert_eq!(&rope.to_string(), "1kkkkz");
    }

    #[test]
    fn test_clear() {
        let mut rope = Rope::new();
        rope.insert(0, "123");
        assert_eq!(rope.len(), 3);
        rope.clear();
        assert_eq!(rope.len(), 0);
        assert_eq!(&rope.to_string(), "");
        rope.insert(0, "kkk");
        assert_eq!(&rope.to_string(), "kkk");
    }

    #[test]
    fn test_insert_many() {
        let mut rope = Rope::new();
        let s = "_12345678_".repeat(10);
        let mut expected = String::new();
        for i in 0..100 {
            expected.insert_str(i, &s);
            rope.insert(i, &s);
            assert_eq!(&rope.to_string(), &expected)
        }
    }

    #[test]
    fn test_repeat_insert() {
        let mut rope = Rope::new();
        rope.insert(0, "123");
        for _ in 0..10000 {
            rope.insert(rope.len() / 2, "k");
        }
    }

    #[test]
    #[ignore]
    fn test_update_1() {
        let mut rope = Rope::new();
        for i in 0..100 {
            rope.insert(i, &(i % 10).to_string());
        }

        rope.update_in_place(15, "kkkkk");
        assert_eq!(&rope.to_string()[10..20], "01234kkkkk");
        test_index(&rope);
    }

    #[derive(Debug)]
    enum Action {
        Insert { pos: u8, content: u8 },
        Delete { pos: u8, len: u8 },
    }

    fn fuzz(data: HeapVec<Action>) {
        let mut rope = Rope::new();
        let mut truth = String::new();
        for action in data {
            match action {
                Action::Insert { pos, content } => {
                    let pos = pos as usize % (truth.len() + 1);
                    let s = content.to_string();
                    dbg!("INS", pos, &s);
                    dbg!(&rope);
                    truth.insert_str(pos, &s);
                    rope.insert(pos, &s);
                    dbg!(&rope);
                    rope.check();
                    assert_eq!(rope.len(), truth.len());
                    assert_eq!(rope.to_string(), truth, "{:#?}", &rope.tree);
                }
                Action::Delete { pos, len } => {
                    let pos = pos as usize % (truth.len() + 1);
                    let mut len = len as usize % 10;
                    len = len.min(truth.len() - pos);
                    dbg!("DEL", pos, len);
                    dbg!(&rope);
                    rope.delete_range(pos..(pos + len));
                    dbg!(&rope);
                    truth.drain(pos..pos + len);
                    rope.check();
                    assert_eq!(rope.len(), truth.len());
                    assert_eq!(rope.to_string(), truth, "{:#?}", &rope.tree);
                }
            }
        }

        assert_eq!(rope.to_string(), truth);
    }

    #[test]
    fn fuzz_0() {
        fuzz(vec![
            Insert {
                pos: 0,
                content: 128,
            },
            Insert {
                pos: 0,
                content: 249,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 192, len: 193 },
            Insert {
                pos: 106,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 100,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 111,
                content: 127,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 36 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 135, len: 169 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
        ])
    }

    #[test]
    fn fuzz_1() {
        fuzz(vec![
            Insert {
                pos: 157,
                content: 108,
            },
            Insert {
                pos: 255,
                content: 255,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 8,
                content: 101,
            },
            Insert {
                pos: 111,
                content: 127,
            },
            Delete { pos: 255, len: 169 },
        ])
    }

    #[test]
    fn fuzz_2() {
        fuzz(vec![
            Insert {
                pos: 0,
                content: 128,
            },
            Insert {
                pos: 0,
                content: 249,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 0,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 249,
            },
            Insert {
                pos: 135,
                content: 255,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 169, len: 169 },
        ])
    }

    #[test]
    fn fuzz_3() {
        fuzz(vec![
            Insert {
                pos: 111,
                content: 140,
            },
            Insert {
                pos: 111,
                content: 107,
            },
            Insert {
                pos: 35,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 0,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 93,
                content: 93,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 102,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 111,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 101,
            },
            Insert {
                pos: 36,
                content: 146,
            },
            Delete { pos: 74, len: 102 },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 17,
                content: 17,
            },
            Insert {
                pos: 17,
                content: 17,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 102,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 111,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 3,
                content: 73,
            },
            Insert {
                pos: 146,
                content: 74,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 21,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 111,
                content: 111,
            },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 3,
            },
            Insert {
                pos: 36,
                content: 146,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Delete { pos: 111, len: 119 },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 73,
                content: 36,
            },
            Delete { pos: 74, len: 102 },
            Delete { pos: 255, len: 255 },
            Insert {
                pos: 42,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 0,
                content: 15,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 3,
            },
            Insert {
                pos: 36,
                content: 146,
            },
            Insert {
                pos: 255,
                content: 255,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 38,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 89,
                content: 89,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 42,
                content: 42,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 37,
            },
            Insert {
                pos: 101,
                content: 102,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 193, len: 63 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 0,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Delete { pos: 199, len: 199 },
            Delete { pos: 199, len: 199 },
            Delete { pos: 199, len: 199 },
            Delete { pos: 199, len: 199 },
            Delete { pos: 199, len: 199 },
            Delete { pos: 199, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Delete { pos: 187, len: 187 },
            Insert {
                pos: 3,
                content: 119,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Delete { pos: 163, len: 163 },
            Delete { pos: 163, len: 163 },
            Delete { pos: 163, len: 102 },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 108,
                content: 249,
            },
            Insert {
                pos: 135,
                content: 169,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 111, len: 255 },
            Insert {
                pos: 111,
                content: 111,
            },
            Insert {
                pos: 255,
                content: 255,
            },
        ])
    }

    #[test]
    fn fuzz_4() {
        fuzz(vec![
            Insert {
                pos: 0,
                content: 128,
            },
            Insert {
                pos: 0,
                content: 249,
            },
            Insert { pos: 8, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 0,
            },
            Insert {
                pos: 108,
                content: 108,
            },
        ])
    }

    #[test]
    fn fuzz_5() {
        fuzz(vec![
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 0,
                content: 123,
            },
            Delete { pos: 108, len: 108 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 12,
                content: 0,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 127,
                content: 135,
            },
            Delete { pos: 255, len: 246 },
            Delete { pos: 246, len: 246 },
            Delete { pos: 246, len: 246 },
            Delete { pos: 246, len: 246 },
            Insert {
                pos: 101,
                content: 101,
            },
            Insert {
                pos: 101,
                content: 101,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 169, len: 169 },
        ])
    }

    #[test]
    fn fuzz_6() {
        fuzz(vec![
            Insert {
                pos: 0,
                content: 128,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 0,
                content: 249,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 171,
                content: 171,
            },
            Delete { pos: 171, len: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 171,
            },
            Delete { pos: 187, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 171,
                content: 171,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 110,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 171,
            },
            Delete { pos: 187, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 8,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 50,
                content: 108,
            },
            Delete { pos: 108, len: 108 },
            Insert {
                pos: 108,
                content: 87,
            },
            Insert {
                pos: 249,
                content: 1,
            },
            Delete { pos: 169, len: 235 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 163, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 8, content: 0 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 41, len: 164 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 2,
            },
            Insert {
                pos: 254,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 0,
                content: 123,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 238,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 238,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 86,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 2,
            },
            Insert {
                pos: 254,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 0,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 238,
                content: 123,
            },
            Delete { pos: 123, len: 123 },
            Insert {
                pos: 86,
                content: 254,
            },
            Insert {
                pos: 33,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 2,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 123, len: 123 },
            Insert {
                pos: 0,
                content: 121,
            },
            Insert {
                pos: 26,
                content: 0,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 238, len: 254 },
            Insert {
                pos: 144,
                content: 238,
            },
            Delete { pos: 91, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 0, len: 51 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 123 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 86,
            },
            Delete { pos: 101, len: 144 },
            Delete { pos: 238, len: 91 },
            Delete { pos: 238, len: 238 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 3, content: 0 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 171,
                content: 63,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 8, content: 0 },
            Insert {
                pos: 127,
                content: 135,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 0, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 0,
                content: 171,
            },
            Delete { pos: 1, len: 126 },
            Delete { pos: 235, len: 154 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 84,
                content: 84,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 91, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 249,
                content: 1,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 108,
                content: 32,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 235, len: 108 },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 255, len: 6 },
            Insert {
                pos: 135,
                content: 169,
            },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 171,
                content: 171,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 171,
                content: 171,
            },
            Insert {
                pos: 126,
                content: 111,
            },
            Delete { pos: 154, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 84,
                content: 171,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 235,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 87,
                content: 0,
            },
            Delete { pos: 1, len: 111 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 86,
                content: 254,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 86,
                content: 0,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 254, len: 193 },
            Delete { pos: 63, len: 64 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 111,
                content: 127,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 0 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 8, len: 0 },
            Delete { pos: 249, len: 1 },
            Delete { pos: 169, len: 235 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 8,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 50,
                content: 108,
            },
            Delete { pos: 108, len: 108 },
            Insert {
                pos: 108,
                content: 8,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 169, len: 235 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 8, content: 0 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 41, len: 164 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 8, content: 0 },
            Insert {
                pos: 171,
                content: 171,
            },
            Insert { pos: 8, content: 0 },
            Insert {
                pos: 127,
                content: 135,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 41, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 41 },
            Insert {
                pos: 171,
                content: 171,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 165, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 170 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 0,
                content: 108,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 171,
                content: 171,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 8, content: 0 },
            Insert {
                pos: 127,
                content: 135,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 123,
                content: 2,
            },
            Insert {
                pos: 254,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 8, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 238,
                content: 238,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 91, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 18 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 86,
                content: 254,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 0,
                content: 123,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 91, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 123 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 86,
                content: 86,
            },
            Insert {
                pos: 202,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 2,
            },
            Insert {
                pos: 254,
                content: 123,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 255, len: 101 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 238,
                content: 123,
            },
            Delete { pos: 123, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 123,
            },
            Insert {
                pos: 238,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 1, len: 0 },
            Insert { pos: 0, content: 7 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 108 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 235,
                content: 235,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 111,
                content: 111,
            },
            Delete { pos: 154, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 171,
                content: 8,
            },
            Delete { pos: 171, len: 249 },
            Insert {
                pos: 135,
                content: 169,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 87,
                content: 84,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 11, len: 238 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 41, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 171, len: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 0,
                content: 108,
            },
            Delete { pos: 63, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 157, len: 157 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 108, len: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 0,
                content: 248,
            },
            Delete { pos: 154, len: 127 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 0 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 8, len: 0 },
            Delete { pos: 249, len: 1 },
            Delete { pos: 169, len: 235 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 84,
                content: 84,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 49,
            },
            Delete { pos: 235, len: 108 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 0,
                content: 249,
            },
            Insert {
                pos: 135,
                content: 169,
            },
            Delete { pos: 238, len: 123 },
            Insert { pos: 2, content: 0 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 1,
            },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 238,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 193,
                content: 192,
            },
            Delete { pos: 63, len: 127 },
            Insert {
                pos: 0,
                content: 235,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 87,
                content: 0,
            },
            Delete { pos: 1, len: 111 },
            Delete { pos: 235, len: 154 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 0, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 127,
                content: 135,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 235, len: 235 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 127,
                content: 135,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 172 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 0 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 0,
                content: 171,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 108 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 235,
                content: 235,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 111,
                content: 111,
            },
            Delete { pos: 171, len: 0 },
            Insert {
                pos: 48,
                content: 111,
            },
            Delete { pos: 154, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Insert {
                pos: 84,
                content: 84,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 235 },
            Delete { pos: 254, len: 86 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 8 },
            Insert {
                pos: 108,
                content: 171,
            },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 20 },
            Delete { pos: 171, len: 108 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 235,
                content: 235,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 111,
                content: 111,
            },
            Delete { pos: 154, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 171, len: 171 },
            Delete { pos: 123, len: 123 },
            Insert {
                pos: 86,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 36,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 254,
                content: 255,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 123 },
            Insert { pos: 0, content: 0 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 123, len: 123 },
            Insert {
                pos: 238,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 238,
                content: 238,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 91, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert { pos: 0, content: 0 },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 238,
                content: 238,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 238, len: 238 },
            Insert {
                pos: 123,
                content: 2,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 238,
                content: 238,
            },
            Insert {
                pos: 0,
                content: 238,
            },
            Delete { pos: 238, len: 238 },
            Delete { pos: 0, len: 249 },
            Insert {
                pos: 135,
                content: 255,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 144, len: 255 },
            Delete { pos: 169, len: 169 },
        ])
    }

    #[test]
    fn ben() {
        use arbitrary::Arbitrary;
        #[derive(Arbitrary, Debug, Clone, Copy)]
        enum Action {
            Insert { pos: u8, content: u8 },
            Delete { pos: u8, len: u8 },
        }

        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);
        let mut expected = String::new();
        let unstructured: Vec<u8> = (0..10_000).map(|_| rng.gen()).collect();
        let mut gen = arbitrary::Unstructured::new(&unstructured);
        let actions: [Action; 1_000] = gen.arbitrary().unwrap();
        let mut rope = Rope::new();
        for action in actions.iter() {
            match *action {
                Action::Insert { pos, content } => {
                    let pos = pos as usize % (rope.len() + 1);
                    let s = content.to_string();
                    expected.insert_str(pos, &s);
                    rope.insert(pos, &s);
                    assert_eq!(expected.len(), rope.len());
                }
                Action::Delete { pos, len } => {
                    let pos = pos as usize % (rope.len() + 1);
                    let mut len = len as usize % 10;
                    len = len.min(rope.len() - pos);
                    expected.drain(pos..pos + len);
                    rope.delete_range(pos..(pos + len));
                    assert_eq!(expected.len(), rope.len());
                }
            }
        }
        assert_eq!(rope.to_string(), expected);
    }

    #[test]
    fn fuzz_7() {
        fuzz(vec![
            Insert {
                pos: 111,
                content: 111,
            },
            Insert { pos: 0, content: 0 },
            Insert { pos: 0, content: 0 },
            Insert { pos: 0, content: 0 },
            Insert { pos: 0, content: 0 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 108,
            },
            Insert {
                pos: 108,
                content: 255,
            },
            Delete { pos: 255, len: 0 },
            Insert {
                pos: 140,
                content: 140,
            },
            Insert {
                pos: 102,
                content: 101,
            },
            Insert {
                pos: 36,
                content: 146,
            },
            Insert {
                pos: 102,
                content: 119,
            },
            Insert {
                pos: 118,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 21,
                content: 0,
            },
            Insert {
                pos: 140,
                content: 140,
            },
            Insert {
                pos: 107,
                content: 19,
            },
            Insert {
                pos: 102,
                content: 47,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 0,
                content: 102,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 0,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Delete { pos: 255, len: 136 },
            Delete { pos: 119, len: 111 },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 3,
                content: 73,
            },
            Insert {
                pos: 146,
                content: 74,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 0, len: 102 },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert { pos: 0, content: 0 },
            Delete { pos: 255, len: 255 },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 0,
                content: 255,
            },
            Delete { pos: 111, len: 108 },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Insert {
                pos: 83,
                content: 108,
            },
            Insert {
                pos: 111,
                content: 111,
            },
            Insert {
                pos: 119,
                content: 21,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 64,
                content: 64,
            },
            Insert {
                pos: 55,
                content: 119,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert { pos: 0, content: 0 },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Delete { pos: 130, len: 130 },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 96,
                content: 102,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 156,
                content: 111,
            },
            Insert {
                pos: 123,
                content: 37,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 37,
                content: 121,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 121,
                content: 86,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 123,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Delete { pos: 239, len: 239 },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 123,
                content: 0,
            },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 255, len: 255 },
            Delete { pos: 125, len: 125 },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 125,
                content: 125,
            },
            Insert {
                pos: 123,
                content: 123,
            },
            Insert {
                pos: 118,
                content: 118,
            },
            Insert {
                pos: 255,
                content: 255,
            },
            Insert {
                pos: 119,
                content: 119,
            },
            Insert {
                pos: 102,
                content: 102,
            },
            Delete { pos: 209, len: 255 },
            Delete { pos: 255, len: 255 },
        ])
    }

    #[test]
    fn from_str() {
        for i in 0..100000 {
            let s = i.to_string();
            let mut g = GapBuffer::from_str(&s);
            assert_eq!(s.len(), g.next().unwrap().len());
        }
    }

    #[test]
    fn from_iter() {
        let mut v = vec![];
        for i in 0..100000 {
            v.push(i.to_string());
        }

        let rope = Rope {
            tree: v
                .iter()
                .flat_map(|x| GapBuffer::from_str(x.as_str()))
                .collect(),
            cursor: None,
        };

        let s = v.join("");
        assert_eq!(rope.to_string(), s);
        assert_eq!(rope.len(), s.len());
        rope.tree.check();
    }

    #[test]
    fn drain() {
        let mut rope = Rope::new();
        for i in 0..100000 {
            rope.insert(0, &i.to_string());
        }

        while !rope.is_empty() {
            let leaf = rope.tree.first_leaf();
            rope.tree.update_leaf(leaf.unwrap(), |elem| {
                elem.slice_(1..1);
                (true, None, None)
            });
        }
    }

    #[test]
    fn fuzz_empty() {
        fuzz(vec![])
    }
}
