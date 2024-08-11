use fxhash::FxHashMap;
use generic_btree::{
    rle::{HasLength, Mergeable},
    LeafIndex,
};
use loro_common::{Counter, IdSpan, PeerID, ID};
use rle::{HasLength as RHasLength, Mergable as RMergeable, Sliceable};
use smallvec::smallvec;
use smallvec::SmallVec;

use self::insert_set::InsertSet;

// If we make this too large, we may have too many cursors inside a fragment
// and trigger the worst case
const MAX_FRAGMENT_LEN: usize = 256;
#[cfg(not(test))]
const SMALL_SET_MAX_LEN: usize = 32;
#[cfg(test)]
const SMALL_SET_MAX_LEN: usize = 4;

/// This struct maintains the mapping of Op `ID` to
///
/// - `LeafIndex` from crdt_rope, if the Op is an Insert
/// - The IdSpan deleted by the Op, if the Op is a Delete
#[derive(Debug, Default)]
pub(super) struct IdToCursor {
    map: FxHashMap<PeerID, Vec<Fragment>>,
}

static EMPTY_VEC: Vec<Fragment> = vec![];
impl IdToCursor {
    pub fn insert_without_split(&mut self, id: ID, cursor: Cursor) {
        let list = self.map.entry(id.peer).or_default();
        list.push(Fragment {
            counter: id.counter,
            cursor,
        });
    }

    pub fn insert(&mut self, id: ID, cursor: Cursor) {
        let list = self.map.entry(id.peer).or_default();
        if let Some(last) = list.last_mut() {
            let last_end = last.counter + last.cursor.rle_len() as Counter;
            debug_assert!(last_end <= id.counter, "id:{}, {:#?}", id, &self);
            if last_end == id.counter
                && last.cursor.can_merge(&cursor)
                && last.cursor.rle_len() + cursor.rle_len() < MAX_FRAGMENT_LEN
            {
                last.cursor.merge_right(&cursor);
                return;
            }
        }

        if let Cursor::Insert(InsertSet::Small(set)) = cursor {
            if set.len > MAX_FRAGMENT_LEN as u32 {
                assert!(set.set.len() == 1);
                let insert = set.set[0];
                let mut counter = id.counter;
                for start in (0..set.len).step_by(MAX_FRAGMENT_LEN) {
                    let end = (start + MAX_FRAGMENT_LEN as u32).min(set.len);
                    let len = (end - start) as usize;
                    list.push(Fragment {
                        counter,
                        cursor: Cursor::new_insert(insert.leaf, len),
                    });
                    counter += len as Counter;
                }
            } else {
                list.push(Fragment {
                    counter: id.counter,
                    cursor: Cursor::Insert(InsertSet::Small(set)),
                });
            }
        } else {
            list.push(Fragment {
                counter: id.counter,
                cursor,
            });
        }
    }

    /// Update the given id_span to the new_leaf
    ///
    /// id_span should be within the same `Cursor` and should be a `Insert`
    pub fn update_insert(&mut self, id_span: IdSpan, new_leaf: LeafIndex) {
        debug_assert!(!id_span.is_reversed());
        let list = self.map.get_mut(&id_span.peer).unwrap();
        let last = list.last().unwrap();
        debug_assert!(last.counter + last.cursor.rle_len() as Counter > id_span.counter.max());
        let mut index = match list.binary_search_by_key(&id_span.counter.start, |x| x.counter) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };

        let mut start_counter = id_span.counter.start;
        while start_counter < id_span.counter.end
            && index < list.len()
            && start_counter < list[index].counter_end()
        {
            let fragment = &mut list[index];
            let from = (start_counter - fragment.counter) as usize;
            let to =
                ((id_span.counter.end - fragment.counter) as usize).min(fragment.cursor.rle_len());

            fragment.cursor.update_insert(from, to, new_leaf);
            start_counter += (to - from) as Counter;
            index += 1;
        }

        assert_eq!(start_counter, id_span.counter.end);
    }

    pub fn iter_all(&self) -> impl Iterator<Item = IterCursor> + '_ {
        self.map.iter().flat_map(|(peer, list)| {
            list.iter()
                .flat_map(move |f| -> Box<dyn Iterator<Item = IterCursor>> {
                    match &f.cursor {
                        Cursor::Insert(set) => set.iter_all(*peer, f.counter),
                        Cursor::Delete(span) => {
                            let start_counter = f.counter;
                            let end_counter = f.counter + span.atom_len() as Counter;
                            let id_span = IdSpan::new(*peer, start_counter, end_counter);
                            Box::new(std::iter::once(IterCursor::Delete(id_span)))
                        }
                        Cursor::Move { from, to } => Box::new(std::iter::once(IterCursor::Move {
                            from_id: *from,
                            to_leaf: *to,
                            new_op_id: ID::new(*peer, f.counter),
                        })),
                    }
                })
        })
    }

    pub fn iter(&self, mut iter_id_span: IdSpan) -> impl Iterator<Item = IterCursor> + '_ {
        iter_id_span.normalize_();
        let list = self.map.get(&iter_id_span.peer).unwrap_or(&EMPTY_VEC);
        // Index in the list
        let mut index = 0;
        let mut insert_set_iter: Option<Box<dyn Iterator<Item = IterCursor>>> = None;

        if !list.is_empty() {
            index = match list.binary_search_by_key(&iter_id_span.counter.start, |x| x.counter) {
                Ok(index) => index,
                Err(index) => index.saturating_sub(1),
            };
        }

        std::iter::from_fn(move || loop {
            if index >= list.len() {
                return None;
            }

            // Always iterate the insert set's iterator first if it exists
            // It's on the top of the stack
            if let Some(iter) = insert_set_iter.as_mut() {
                let Some(next) = iter.next() else {
                    index += 1;
                    insert_set_iter = None;
                    continue;
                };

                return Some(next);
            }

            let f = &list[index];
            let iter_counter = f.counter;
            if iter_counter >= iter_id_span.counter.end {
                return None;
            }

            if iter_counter + f.cursor.rle_len() as Counter <= iter_id_span.counter.start {
                index += 1;
                continue;
            }

            match &f.cursor {
                Cursor::Insert(set) => {
                    insert_set_iter = Some(set.iter_range(
                        ID::new(iter_id_span.peer, iter_counter),
                        iter_id_span.counter.start,
                        iter_id_span.counter.end,
                    ));
                    continue;
                }
                Cursor::Delete(span) => {
                    index += 1;
                    let from = (iter_id_span.counter.start - iter_counter)
                        .max(0)
                        .min(span.atom_len() as Counter);
                    let to = (iter_id_span.counter.end - iter_counter)
                        .max(0)
                        .min(span.atom_len() as Counter);
                    if from == to {
                        continue;
                    }

                    return Some(IterCursor::Delete(span.slice(from as usize, to as usize)));
                }
                Cursor::Move { from, to } => {
                    index += 1;
                    let op_id = ID::new(iter_id_span.peer, f.counter);
                    return Some(IterCursor::Move {
                        from_id: *from,
                        to_leaf: *to,
                        new_op_id: op_id,
                    });
                }
            }
        })
    }

    pub fn get_insert(&self, id: ID) -> Option<LeafIndex> {
        let list = self.map.get(&id.peer)?;
        let index = match list.binary_search_by_key(&id.counter, |x| x.counter) {
            Ok(index) => index,
            Err(index) => index - 1,
        };

        list[index]
            .cursor
            .get_insert((id.counter - list[index].counter) as usize)
    }

    #[allow(unused)]
    pub fn diagnose(&self) {
        let fragment_num = self.map.iter().map(|x| x.1.len()).sum::<usize>();
        let insert_pieces = self
            .map
            .iter()
            .flat_map(|x| x.1.iter())
            .map(|x| match &x.cursor {
                Cursor::Insert(set) => set.len(),
                Cursor::Delete(_) => 0,
                Cursor::Move { .. } => 0,
            })
            .sum::<usize>();
        let max_insert_len = self
            .map
            .iter()
            .map(|x| {
                x.1.iter()
                    .filter_map(|x| match &x.cursor {
                        Cursor::Insert(set) => Some(set.len()),
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        println!(
            "fragments:{}, insert_pieces:{}, max_insert_len:{}",
            fragment_num, insert_pieces, max_insert_len
        );
    }
}

#[derive(Debug)]
pub(super) struct Fragment {
    pub(super) counter: Counter,
    pub(super) cursor: Cursor,
}

impl PartialEq for Fragment {
    fn eq(&self, other: &Self) -> bool {
        self.counter == other.counter
    }
}

impl Eq for Fragment {}

impl PartialOrd for Fragment {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fragment {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.counter.cmp(&other.counter)
    }
}

impl Fragment {
    fn counter_end(&self) -> Counter {
        self.counter + self.cursor.rle_len() as Counter
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum IterCursor {
    Insert {
        leaf: LeafIndex,
        id_span: IdSpan,
    },
    // deleted id_span, the start may be greater than the end
    Delete(IdSpan),
    // Move from `from ID` to `to LeafIndex` with `op_id`
    Move {
        from_id: ID,
        to_leaf: LeafIndex,
        new_op_id: ID,
    },
}

impl IterCursor {
    fn len(&self) -> usize {
        match self {
            IterCursor::Insert { id_span, .. } => id_span.atom_len(),
            IterCursor::Delete(id_span) => id_span.atom_len(),
            IterCursor::Move { .. } => 1,
        }
    }
}

#[derive(Debug)]
pub(super) enum Cursor {
    Insert(InsertSet),
    Delete(IdSpan),
    Move {
        from: ID,
        to: LeafIndex,
        // to id is the same as the current op_id
    },
}

mod insert_set {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use generic_btree::{
        rle::{CanRemove, TryInsert},
        BTree, BTreeTrait, LengthFinder, UseLengthFinder,
    };
    use smallvec::SmallVec;

    #[derive(Debug)]
    pub(super) enum InsertSet {
        Small(SmallInsertSet),
        Large(LargeInsertSet),
    }

    impl InsertSet {
        pub(crate) fn new(leaf: LeafIndex, len: usize) -> Self {
            Self::Small(SmallInsertSet {
                set: smallvec![Insert {
                    leaf,
                    len: len as u32
                }],
                len: len as u32,
            })
        }

        pub(crate) fn update(&mut self, from: usize, to: usize, new_leaf: LeafIndex) {
            self.upgrade_if_needed();
            match self {
                Self::Small(set) => {
                    set.update(from, to, new_leaf);
                }
                Self::Large(set) => {
                    set.update(from, to, new_leaf);
                }
            }
        }

        pub(crate) fn get_insert(&self, pos: usize) -> Option<LeafIndex> {
            match self {
                Self::Small(set) => {
                    let mut index = 0;
                    for insert in set.set.iter() {
                        if index + insert.len as usize > pos {
                            return Some(insert.leaf);
                        }
                        index += insert.len as usize;
                    }

                    unreachable!()
                }
                Self::Large(set) => set.get_insert(pos),
            }
        }

        pub(crate) fn len(&self) -> usize {
            match self {
                Self::Small(set) => set.len as usize,
                Self::Large(set) => *set.tree.root_cache() as usize,
            }
        }

        pub(crate) fn iter_all(
            &self,
            peer: PeerID,
            counter: Counter,
        ) -> Box<dyn Iterator<Item = IterCursor> + '_> {
            match self {
                InsertSet::Small(set) => {
                    let mut offset = 0;
                    Box::new(set.set.iter().map(move |elem| {
                        let ans = IterCursor::Insert {
                            leaf: elem.leaf,
                            id_span: IdSpan::new(
                                peer,
                                counter + offset as Counter,
                                counter + offset as Counter + elem.len as Counter,
                            ),
                        };

                        offset += elem.len;
                        ans
                    }))
                }
                InsertSet::Large(set) => {
                    let mut offset = 0;
                    Box::new(set.tree.iter().map(move |elem| {
                        let ans = IterCursor::Insert {
                            leaf: elem.leaf,
                            id_span: IdSpan::new(
                                peer,
                                counter + offset as Counter,
                                counter + offset as Counter + elem.len as Counter,
                            ),
                        };

                        offset += elem.len;
                        ans
                    }))
                }
            }
        }

        /// Iterate the given target span range, the start id of the InsertSet is cur_id
        pub(crate) fn iter_range(
            &self,
            cur_id: ID,
            target_start_counter: i32,
            target_end_counter: i32,
        ) -> Box<dyn Iterator<Item = IterCursor> + '_> {
            match self {
                InsertSet::Small(set) => {
                    let mut offset = 0;
                    Box::new(set.set.iter().filter_map(move |elem| {
                        let id_span = IdSpan::new(
                            cur_id.peer,
                            (cur_id.counter + offset as Counter)
                                .max(target_start_counter)
                                .min(target_end_counter),
                            (cur_id.counter + offset as Counter + elem.len as Counter)
                                .max(target_start_counter)
                                .min(target_end_counter),
                        );

                        offset += elem.len;
                        if id_span.atom_len() == 0 {
                            return None;
                        }

                        let ans = IterCursor::Insert {
                            leaf: elem.leaf,
                            id_span,
                        };

                        Some(ans)
                    }))
                }
                InsertSet::Large(set) => {
                    // let mut offset = 0;
                    // Box::new(set.tree.iter().filter_map(move |elem| {
                    //     let id_span = IdSpan::new(
                    //         cur_id.peer,
                    //         (cur_id.counter + offset as Counter)
                    //             .max(target_start_counter)
                    //             .min(target_end_counter),
                    //         (cur_id.counter + offset as Counter + elem.len as Counter)
                    //             .max(target_start_counter)
                    //             .min(target_end_counter),
                    //     );

                    //     offset += elem.len;
                    //     if id_span.atom_len() == 0 {
                    //         return None;
                    //     }

                    //     let ans = IterCursor::Insert {
                    //         leaf: elem.leaf,
                    //         id_span,
                    //     };

                    //     Some(ans)
                    // }));
                    let offset = (target_start_counter - cur_id.counter).max(0);
                    let (start, mut start_counter) = if offset > 0 {
                        match set.tree.query::<LengthFinder>(&(offset as usize)) {
                            Some(start) => (
                                std::ops::Bound::Included(start.cursor),
                                // NOTE: Can this be wrong?
                                target_start_counter - start.cursor.offset as Counter,
                            ),
                            _ => (std::ops::Bound::Unbounded, cur_id.counter),
                        }
                    } else {
                        (std::ops::Bound::Unbounded, cur_id.counter)
                    };

                    Box::new(
                        set.tree
                            .iter_range((start, std::ops::Bound::Unbounded))
                            .map(move |b| {
                                let id_span = IdSpan::new(
                                    cur_id.peer,
                                    (start_counter)
                                        .max(target_start_counter)
                                        .min(target_end_counter),
                                    (start_counter + b.elem.rle_len() as Counter)
                                        .max(target_start_counter)
                                        .min(target_end_counter),
                                );

                                start_counter += b.elem.rle_len() as Counter;
                                if id_span.atom_len() == 0 {
                                    return None;
                                }

                                let ans = IterCursor::Insert {
                                    leaf: b.elem.leaf,
                                    id_span,
                                };

                                Some(ans)
                            })
                            .take_while(|x| x.is_some())
                            .map(|x| x.unwrap()),
                    )
                }
            }
        }

        fn upgrade_if_needed(&mut self) {
            match self {
                InsertSet::Small(set) => {
                    if set.set.len() < SMALL_SET_MAX_LEN {
                        return;
                    }

                    let set = std::mem::take(&mut set.set);
                    let tree_set = LargeInsertSet::init_from_small(set);
                    *self = InsertSet::Large(tree_set);
                }
                InsertSet::Large(_) => {}
            }
        }
    }

    #[derive(Debug)]
    pub(super) struct SmallInsertSet {
        pub set: SmallVec<[Insert; 1]>,
        pub len: u32,
    }

    impl SmallInsertSet {
        fn update(&mut self, from: usize, to: usize, new_leaf: LeafIndex) {
            let mut cur_scan_index: usize = 0;
            let mut new_set = SmallVec::new();
            let mut new_leaf_inserted = false;
            for insert in self.set.iter() {
                if new_leaf_inserted {
                    let end = cur_scan_index + insert.len as usize;
                    if end <= to {
                        cur_scan_index = end;
                        continue;
                    }

                    if cur_scan_index >= to {
                        new_set.push(*insert);
                    } else {
                        new_set.push(Insert {
                            leaf: insert.leaf,
                            len: (end - to) as u32,
                        });
                    }

                    cur_scan_index = end;
                    continue;
                }

                if cur_scan_index + insert.len as usize <= from {
                    new_set.push(*insert);
                    cur_scan_index += insert.len as usize;
                } else {
                    debug_assert!(!new_leaf_inserted);
                    let elem_end = cur_scan_index + insert.len as usize;
                    if cur_scan_index < from {
                        new_set.push(Insert {
                            leaf: insert.leaf,
                            len: (from - cur_scan_index) as u32,
                        });
                        cur_scan_index = from;
                    }

                    if elem_end > to {
                        new_set.push(Insert {
                            leaf: new_leaf,
                            len: (to - cur_scan_index) as u32,
                        });
                        new_set.push(Insert {
                            leaf: insert.leaf,
                            len: (elem_end - to) as u32,
                        });
                    } else {
                        new_set.push(Insert {
                            leaf: new_leaf,
                            len: (to - cur_scan_index) as u32,
                        });
                    }

                    new_leaf_inserted = true;
                    cur_scan_index = elem_end;
                }
            }

            self.set = new_set;
            debug_assert_eq!(
                self.len,
                self.set.iter().map(|x| x.len as usize).sum::<usize>() as u32
            );
        }
    }

    pub(super) struct InsertSetBTreeTrait;
    impl CanRemove for Insert {
        fn can_remove(&self) -> bool {
            self.len == 0
        }
    }

    impl generic_btree::rle::Mergeable for Insert {
        fn can_merge(&self, rhs: &Self) -> bool {
            self.leaf == rhs.leaf
        }

        fn merge_right(&mut self, rhs: &Self) {
            self.len += rhs.len;
        }

        fn merge_left(&mut self, left: &Self) {
            self.len += left.len;
        }
    }

    impl HasLength for Insert {
        fn rle_len(&self) -> usize {
            self.len as usize
        }
    }

    impl generic_btree::rle::Sliceable for Insert {
        fn _slice(&self, range: std::ops::Range<usize>) -> Self {
            Insert {
                leaf: self.leaf,
                len: range.len() as u32,
            }
        }
    }

    impl TryInsert for Insert {
        fn try_insert(&mut self, _: usize, elem: Self) -> Result<(), Self> {
            if self.leaf == elem.leaf {
                self.len += elem.len;
                Ok(())
            } else {
                Err(elem)
            }
        }
    }

    impl BTreeTrait for InsertSetBTreeTrait {
        type Elem = Insert;

        type Cache = i32;

        type CacheDiff = i32;

        fn calc_cache_internal(
            cache: &mut Self::Cache,
            caches: &[generic_btree::Child<Self>],
        ) -> Self::CacheDiff {
            let new: Self::Cache = caches.iter().map(|x| *x.cache()).sum();
            let diff = new - *cache;
            *cache = new;
            diff
        }

        fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
            *cache += *diff;
        }

        fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
            *diff1 += *diff2;
        }

        fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
            elem.len as i32
        }

        fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
            *cache
        }

        fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
            *cache_lhs - *cache_rhs
        }
    }

    impl UseLengthFinder<InsertSetBTreeTrait> for InsertSetBTreeTrait {
        fn get_len(cache: &i32) -> usize {
            *cache as usize
        }
    }

    #[derive(Debug)]
    pub(super) struct LargeInsertSet {
        tree: Box<BTree<InsertSetBTreeTrait>>,
    }

    impl LargeInsertSet {
        pub fn new() -> Self {
            Self {
                tree: Box::new(BTree::new()),
            }
        }

        fn update(&mut self, from: usize, to: usize, new_leaf: LeafIndex) {
            let Some(from) = self.tree.query::<LengthFinder>(&from) else {
                return;
            };
            let Some(to) = self.tree.query::<LengthFinder>(&to) else {
                return;
            };

            self.tree.update(from.cursor..to.cursor, &mut |x| {
                x.leaf = new_leaf;
                None
            });
        }

        fn get_insert(&self, pos: usize) -> Option<LeafIndex> {
            let c = self.tree.query::<LengthFinder>(&pos)?.cursor;
            Some(self.tree.get_elem(c.leaf)?.leaf)
        }

        fn init_from_small(set: SmallVec<[Insert; 1]>) -> LargeInsertSet {
            let mut tree_set = LargeInsertSet::new();
            for item in set.into_iter() {
                tree_set.tree.push(item);
            }

            tree_set
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Insert {
    leaf: LeafIndex,
    len: u32,
}

impl Cursor {
    pub fn new_insert(leaf: LeafIndex, len: usize) -> Self {
        Self::Insert(InsertSet::new(leaf, len))
    }

    pub fn new_move(leaf: LeafIndex, from_id: ID) -> Self {
        Self::Move {
            to: leaf,
            from: from_id,
        }
    }

    #[allow(unused)]
    pub fn new_delete(id_span: IdSpan) -> Self {
        Self::Delete(id_span)
    }

    fn update_insert(&mut self, from: usize, to: usize, new_leaf: LeafIndex) {
        // tracing::info!(
        //     "set_insert: from={}, to={}, new_leaf={:?}",
        //     from,
        //     to,
        //     &new_leaf
        // );

        assert!(from <= to);
        assert!(to <= self.rle_len());
        match self {
            Self::Insert(set) => {
                set.update(from, to, new_leaf);
            }
            Self::Move { .. } => {
                unreachable!("update_insert on Move")
            }
            _ => unreachable!(),
        }
    }

    fn get_insert(&self, pos: usize) -> Option<LeafIndex> {
        if pos >= self.rle_len() {
            return None;
        }

        match self {
            Cursor::Insert(set) => set.get_insert(pos),
            Cursor::Move { to, .. } => {
                assert!(pos == 0);
                Some(*to)
            }
            Cursor::Delete(_) => unreachable!(),
        }
    }
}

impl HasLength for Cursor {
    fn rle_len(&self) -> usize {
        match self {
            Cursor::Insert(set) => set.len(),
            Cursor::Delete(d) => d.atom_len(),
            Cursor::Move { .. } => 1,
        }
    }
}

impl Mergeable for Cursor {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Self::Insert(InsertSet::Small(a)), Self::Insert(InsertSet::Small(b))) => {
                a.set.last().unwrap().leaf == b.set.first().unwrap().leaf && b.len == 1
            }
            (Self::Delete(a), Self::Delete(b)) => a.is_mergable(b, &()),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (Self::Insert(InsertSet::Small(a)), Self::Insert(InsertSet::Small(b))) => {
                assert!(b.len == 1);
                a.set.last_mut().unwrap().len += b.set.first().unwrap().len;
                a.len += b.len;
            }
            (Self::Delete(a), Self::Delete(b)) => {
                a.merge(b, &());
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, _: &Self) {
        unreachable!();
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_id_to_cursor() {
        let _map = IdToCursor::default();
    }
}
