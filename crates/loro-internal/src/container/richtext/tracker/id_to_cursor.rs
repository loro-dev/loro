use fxhash::FxHashMap;
use generic_btree::{
    rle::{HasLength, Mergeable},
    LeafIndex,
};
use loro_common::{Counter, IdSpan, PeerID, ID};
use rle::{HasLength as RHasLength, Mergable as RMergeable, Sliceable};
use smallvec::smallvec;
use smallvec::SmallVec;

const MAX_FRAGMENT_LEN: usize = 256;

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
    pub fn insert(&mut self, id: ID, cursor: Cursor) {
        let list = self.map.entry(id.peer).or_default();
        if let Some(last) = list.last_mut() {
            let last_end = last.counter + last.cursor.rle_len() as Counter;
            debug_assert!(last_end <= id.counter, "id:{}, {:#?}", id, &self);
            if last_end == id.counter
                && last.cursor.can_merge(&cursor)
                && last.cursor.rle_len() < MAX_FRAGMENT_LEN
            {
                last.cursor.merge_right(&cursor);
                return;
            }
        }

        list.push(Fragment {
            counter: id.counter,
            cursor,
        });
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
                        Cursor::Insert { set, len: _ } => {
                            let mut offset = 0;
                            Box::new(set.iter().map(move |elem| {
                                let ans = IterCursor::Insert {
                                    leaf: elem.leaf,
                                    id_span: IdSpan::new(
                                        *peer,
                                        f.counter + offset as Counter,
                                        f.counter + offset as Counter + elem.len as Counter,
                                    ),
                                };
                                offset += elem.len;
                                ans
                            }))
                        }
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
        let mut index = 0;
        let mut offset_in_insert_set = 0;
        let mut counter = 0;

        if !list.is_empty() {
            index = match list.binary_search_by_key(&iter_id_span.counter.start, |x| x.counter) {
                Ok(index) => index,
                Err(index) => index.saturating_sub(1),
            };

            offset_in_insert_set = 0;
            counter = list[index].counter;
        }

        std::iter::from_fn(move || loop {
            if index >= list.len() || counter >= iter_id_span.counter.end {
                return None;
            }

            let f = &list[index];
            match &f.cursor {
                Cursor::Insert { set, len: _ } => {
                    if offset_in_insert_set == set.len() {
                        index += 1;
                        offset_in_insert_set = 0;
                        counter = list.get(index).map(|x| x.counter).unwrap_or(Counter::MAX);
                        continue;
                    }

                    offset_in_insert_set += 1;
                    let start_counter = counter;
                    let elem = set[offset_in_insert_set - 1];
                    counter += elem.len as Counter;
                    let end_counter = counter;
                    if end_counter <= iter_id_span.counter.start {
                        continue;
                    }

                    return Some(IterCursor::Insert {
                        leaf: elem.leaf,
                        id_span: IdSpan::new(
                            iter_id_span.peer,
                            start_counter
                                .max(iter_id_span.counter.start)
                                .min(iter_id_span.counter.end),
                            end_counter
                                .max(iter_id_span.counter.start)
                                .min(iter_id_span.counter.end),
                        ),
                    });
                }
                Cursor::Delete(span) => {
                    offset_in_insert_set = 0;
                    index += 1;
                    let start_counter = counter;
                    counter = list.get(index).map(|x| x.counter).unwrap_or(Counter::MAX);
                    if counter <= iter_id_span.counter.start {
                        continue;
                    }

                    let from = (iter_id_span.counter.start - start_counter)
                        .max(0)
                        .min(span.atom_len() as Counter);
                    let to = (iter_id_span.counter.end - start_counter)
                        .max(0)
                        .min(span.atom_len() as Counter);
                    if from == to {
                        continue;
                    }

                    return Some(IterCursor::Delete(span.slice(from as usize, to as usize)));
                }
                Cursor::Move { from, to } => {
                    offset_in_insert_set = 0;
                    index += 1;
                    counter = list.get(index).map(|x| x.counter).unwrap_or(Counter::MAX);
                    let op_id = ID::new(iter_id_span.peer, f.counter);
                    debug_assert!(iter_id_span.contains(op_id));
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
                Cursor::Insert { set, len } => set.len(),
                Cursor::Delete(_) => 0,
                Cursor::Move { .. } => 0,
            })
            .sum::<usize>();
        eprintln!(
            "fragments:{}, insert_pieces:{}",
            fragment_num, insert_pieces
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

#[derive(Debug)]
pub(super) enum Cursor {
    Insert {
        set: SmallVec<[Insert; 1]>,
        len: u32,
    },
    Delete(IdSpan),
    Move {
        from: ID,
        to: LeafIndex,
        // to id is the same as the current op_id
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Insert {
    leaf: LeafIndex,
    len: u32,
}

impl Cursor {
    pub fn new_insert(leaf: LeafIndex, len: usize) -> Self {
        Self::Insert {
            set: smallvec![Insert {
                leaf,
                len: len as u32
            }],
            len: len as u32,
        }
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
            Self::Insert { set, len } => {
                // TODO: PERF can be speed up
                let mut cur_scan_index: usize = 0;
                let mut new_set = SmallVec::new();
                let mut new_leaf_inserted = false;
                for insert in set.iter() {
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

                *set = new_set;
                debug_assert_eq!(
                    *len,
                    set.iter().map(|x| x.len as usize).sum::<usize>() as u32
                );
            }
            Self::Move { from: _, to: leaf } => {
                assert!(to == 1 && from == 0);
                *leaf = new_leaf;
            }
            _ => unreachable!(),
        }
    }

    fn get_insert(&self, pos: usize) -> Option<LeafIndex> {
        if pos >= self.rle_len() {
            return None;
        }

        match self {
            Cursor::Insert { set, len: _ } => {
                let mut index = 0;
                for insert in set.iter() {
                    if index + insert.len as usize > pos {
                        return Some(insert.leaf);
                    }
                    index += insert.len as usize;
                }

                unreachable!()
            }
            Cursor::Delete(_) => unreachable!(),
            Cursor::Move { .. } => unreachable!(),
        }
    }
}

impl HasLength for Cursor {
    fn rle_len(&self) -> usize {
        match self {
            Cursor::Insert { set: _, len } => *len as usize,
            Cursor::Delete(d) => d.atom_len(),
            Cursor::Move { .. } => 1,
        }
    }
}

impl Mergeable for Cursor {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Self::Insert { set: a, .. }, Self::Insert { set: b, .. }) => {
                a.last().unwrap().leaf == b.first().unwrap().leaf && b.len() == 1
            }
            (Self::Delete(a), Self::Delete(b)) => a.is_mergable(b, &()),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (Self::Insert { set: a, len: a_len }, Self::Insert { set: b, len: b_len }) => {
                assert!(b.len() == 1);
                a.last_mut().unwrap().len += b.first().unwrap().len;
                *a_len += *b_len;
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
