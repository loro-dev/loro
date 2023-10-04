use fxhash::FxHashMap;
use generic_btree::{
    rle::{HasLength, Mergeable},
    LeafIndex,
};
use loro_common::{Counter, IdSpan, PeerID, ID};
use rle::{HasLength as RHasLength, Mergable as RMergeable};
use smallvec::smallvec;
use smallvec::SmallVec;
use std::collections::BTreeSet;

const MAX_FRAGMENT_LEN: usize = 256;

/// This struct maintains the mapping of Op `ID` to
///
/// - `LeafIndex` from crdt_rope, if the Op is an Insert
/// - The IdSpan deleted by the Op, if the Op is a Delete
#[derive(Debug, Default)]
pub(super) struct IdToCursor {
    map: FxHashMap<PeerID, Vec<Fragment>>,
}

impl IdToCursor {
    pub fn push(&mut self, id: ID, cursor: Cursor) {
        let list = self.map.entry(id.peer).or_default();
        if let Some(last) = list.last_mut() {
            assert_eq!(last.counter + last.cursor.rle_len() as Counter, id.counter);
            if last.cursor.can_merge(&cursor) && last.cursor.rle_len() < MAX_FRAGMENT_LEN {
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
        let list = self.map.get_mut(&id_span.client_id).unwrap();
        let index = match list.binary_search_by_key(&id_span.counter.start, |x| x.counter) {
            Ok(index) => index,
            Err(index) => index - 1,
        };

        let fragment = &mut list[index];

        fragment.cursor.set_insert(
            (id_span.counter.start - fragment.counter) as usize,
            (id_span.counter.end - fragment.counter) as usize,
            new_leaf,
        )
    }

    pub fn iter(&self, id_span: IdSpan) -> impl Iterator<Item = &Fragment> + '_ {
        let list = self.map.get(&id_span.client_id).unwrap();
        let index = match list.binary_search_by_key(&id_span.counter.start, |x| x.counter) {
            Ok(index) => index,
            Err(index) => index - 1,
        };

        list[index..]
            .iter()
            .take_while(move |x| x.counter < id_span.counter.end)
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
        self.counter.partial_cmp(&other.counter)
    }
}

impl Ord for Fragment {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.counter.cmp(&other.counter)
    }
}

#[derive(Debug)]
pub(super) enum Cursor {
    Insert {
        set: SmallVec<[Insert; 1]>,
        len: u32,
    },
    Delete(IdSpan),
}

#[derive(Debug)]
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

    pub fn new_delete(id_span: IdSpan) -> Self {
        Self::Delete(id_span)
    }

    fn set_insert(&mut self, from: usize, to: usize, new_leaf: LeafIndex) {
        match self {
            Self::Insert { set, len } => {
                let mut index = 0;
                let mut pos = usize::MAX;
                for (i, insert) in set.iter_mut().enumerate() {
                    if index + insert.len as usize > from {
                        pos = i;
                        insert.len -= (to - from) as u32;
                        break;
                    }
                    index += insert.len as usize;
                }

                set.insert(
                    pos + 1,
                    Insert {
                        leaf: new_leaf,
                        len: (to - from) as u32,
                    },
                );
            }
            _ => unreachable!(),
        }
    }

    fn get_insert(&self, pos: usize) -> Option<LeafIndex> {
        if pos >= self.rle_len() {
            return None;
        }

        match self {
            Cursor::Insert { set, len } => {
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
        }
    }
}

impl HasLength for Cursor {
    fn rle_len(&self) -> usize {
        match self {
            Cursor::Insert { set: _, len } => *len as usize,
            Cursor::Delete(d) => d.atom_len(),
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
