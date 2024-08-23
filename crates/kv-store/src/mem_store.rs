use crate::iter::BlockIter;
use crate::sstable::{SsTable, SsTableBuilder, SsTableIter};
use bytes::Bytes;
use fxhash::FxHashSet;

use std::ops::Bound;
use std::{cmp::Ordering, collections::BTreeMap};

const DEFAULT_BLOCK_SIZE: usize = 4 * 1024;

#[derive(Debug, Clone)]
pub struct MemKvStore {
    mem_table: BTreeMap<Bytes, Bytes>,
    ss_table: Option<SsTable>,
    block_size: usize,
}

impl Default for MemKvStore {
    fn default() -> Self {
        Self::new(DEFAULT_BLOCK_SIZE)
    }
}

impl MemKvStore {
    pub fn new(block_size: usize) -> Self {
        Self {
            mem_table: BTreeMap::new(),
            ss_table: None,
            block_size,
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        if let Some(v) = self.mem_table.get(key) {
            if v.is_empty() {
                return None;
            }
            return Some(v.clone());
        }

        if let Some(table) = &self.ss_table {
            if table.first_key > key || table.last_key < key {
                return None;
            }

            // table.
            let idx = table.find_block_idx(key);
            let block = table.read_block_cached(idx);
            let block_iter = BlockIter::new_seek_to_key(block, key);
            if block_iter.next_is_valid() && block_iter.next_curr_key() == key {
                Some(block_iter.next_curr_value())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn set(&mut self, key: &[u8], value: Bytes) {
        self.mem_table.insert(Bytes::copy_from_slice(key), value);
    }

    pub fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool {
        match self.get(key) {
            Some(v) => {
                if old == Some(v) {
                    self.set(key, new);
                    true
                } else {
                    false
                }
            }
            None => {
                if old.is_none() {
                    self.set(key, new);
                    true
                } else {
                    false
                }
            }
        }
    }

    pub fn remove(&mut self, key: &[u8]) {
        self.set(key, Bytes::new());
    }

    pub fn contains_key(&self, key: &[u8]) -> bool {
        if self.mem_table.contains_key(key) {
            return !self.mem_table.get(key).unwrap().is_empty();
        }
        if let Some(table) = &self.ss_table {
            return table.contains_key(key);
        }
        false
    }

    pub fn scan(
        &self,
        start: std::ops::Bound<&[u8]>,
        end: std::ops::Bound<&[u8]>,
    ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_> {
        if self.ss_table.is_none() || self.ss_table.as_ref().unwrap().meta_len() == 0 {
            return Box::new(
                self.mem_table
                    .range::<[u8], _>((start, end))
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }
        if self.mem_table.is_empty() {
            return Box::new(SsTableIter::new_scan(
                self.ss_table.as_ref().unwrap(),
                start,
                end,
            ));
        }
        Box::new(MergeIterator::new(
            self.mem_table
                .range::<[u8], _>((start, end))
                .map(|(k, v)| (k.clone(), v.clone())),
            SsTableIter::new_scan(self.ss_table.as_ref().unwrap(), start, end),
        ))
    }

    pub fn len(&self) -> usize {
        let deleted = self
            .mem_table
            .iter()
            .filter(|(_, v)| v.is_empty())
            .map(|(k, _)| k.clone())
            .collect::<FxHashSet<Bytes>>();
        let default_keys = FxHashSet::default();
        let ss_keys = self
            .ss_table
            .as_ref()
            .map_or(&default_keys, |table| table.valid_keys());
        let ss_len = ss_keys
            .difference(&self.mem_table.keys().cloned().collect())
            .count();
        self.mem_table.len() + ss_len - deleted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mem_table.is_empty() && self.ss_table.is_none()
    }

    pub fn size(&self) -> usize {
        self.mem_table
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + v.len())
            + self.ss_table.as_ref().map_or(0, |table| table.data_size())
    }

    pub fn export_all(&mut self) -> Bytes {
        let mut builder = SsTableBuilder::new(self.block_size);
        for (k, v) in self.scan(Bound::Unbounded, Bound::Unbounded) {
            builder.add(k, v);
        }
        builder.finish_block();

        if builder.is_empty() {
            return Bytes::new();
        }
        self.mem_table.clear();
        let ss = builder.build();
        let ans = ss.export_all();
        let _ = std::mem::replace(&mut self.ss_table, Some(ss));

        ans
    }

    pub fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
        if bytes.is_empty() {
            self.ss_table = None;
            return Ok(());
        }
        let ss_table = SsTable::import_all(bytes).map_err(|e| e.to_string())?;
        self.ss_table = Some(ss_table);
        Ok(())
    }
}

#[derive(Debug)]
struct MergeIterator<'a, T> {
    a: T,
    b: SsTableIter<'a>,
    current_btree: Option<(Bytes, Bytes)>,
    current_sstable: Option<(Bytes, Bytes)>,
    back_btree: Option<(Bytes, Bytes)>,
    back_sstable: Option<(Bytes, Bytes)>,
}

impl<'a, T: DoubleEndedIterator<Item = (Bytes, Bytes)>> MergeIterator<'a, T> {
    fn new(mut a: T, b: SsTableIter<'a>) -> Self {
        let current_btree = a.next();
        let back_btree = a.next_back();
        Self {
            a,
            b,
            current_btree,
            back_btree,
            current_sstable: None,
            back_sstable: None,
        }
    }
}

impl<'a, T: DoubleEndedIterator<Item = (Bytes, Bytes)>> Iterator for MergeIterator<'a, T> {
    type Item = (Bytes, Bytes);
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_sstable.is_none() && self.b.is_next_valid() {
            self.current_sstable = Some((self.b.next_key(), self.b.next_value()));
            self.b.next();
        }

        if self.current_btree.is_none() && self.back_btree.is_some() {
            std::mem::swap(&mut self.back_btree, &mut self.current_btree);
        }
        let ans = match (&self.current_btree, &self.current_sstable) {
            (Some((btree_key, _)), Some((iter_key, _))) => match btree_key.cmp(iter_key) {
                Ordering::Less => self.current_btree.take().map(|kv| {
                    self.current_btree = self.a.next();
                    kv
                }),
                Ordering::Equal => {
                    self.current_sstable.take();
                    self.current_btree.take().map(|kv| {
                        self.current_btree = self.a.next();
                        kv
                    })
                }
                Ordering::Greater => self.current_sstable.take(),
            },
            (Some(_), None) => self.current_btree.take().map(|kv| {
                self.current_btree = self.a.next();
                kv
            }),
            (None, Some(_)) => self.current_sstable.take(),
            (None, None) => None,
        };

        if let Some((_k, v)) = &ans {
            if v.is_empty() {
                return self.next();
            }
        }
        ans
    }
}

impl<'a, T: DoubleEndedIterator<Item = (Bytes, Bytes)>> DoubleEndedIterator
    for MergeIterator<'a, T>
{
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.back_sstable.is_none() && self.b.is_prev_valid() {
            self.back_sstable = Some((self.b.prev_key(), self.b.prev_value()));
            self.b.next_back();
        }

        if self.back_btree.is_none() && self.current_btree.is_some() {
            std::mem::swap(&mut self.back_btree, &mut self.current_btree);
        }

        let ans = match (&self.back_btree, &self.back_sstable) {
            (Some((btree_key, _)), Some((iter_key, _))) => match btree_key.cmp(iter_key) {
                Ordering::Greater => self.back_btree.take().map(|kv| {
                    self.back_btree = self.a.next_back();
                    kv
                }),
                Ordering::Equal => {
                    self.back_sstable.take();
                    self.back_btree.take().map(|kv| {
                        self.back_btree = self.a.next_back();
                        kv
                    })
                }
                Ordering::Less => self.back_sstable.take(),
            },
            (Some(_), None) => self.back_btree.take().map(|kv| {
                self.back_btree = self.a.next_back();
                kv
            }),
            (None, Some(_)) => self.back_sstable.take(),
            (None, None) => None,
        };
        if let Some((_k, v)) = &ans {
            if v.is_empty() {
                return self.next_back();
            }
        }
        ans
    }
}
