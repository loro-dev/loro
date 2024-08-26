use crate::block::BlockIter;
use crate::compress::CompressionType;
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
    compression_type: CompressionType,
}

impl Default for MemKvStore {
    fn default() -> Self {
        Self::new(DEFAULT_BLOCK_SIZE, CompressionType::LZ4)
    }
}

impl MemKvStore {
    pub fn new(block_size: usize, compression_type: CompressionType) -> Self {
        Self {
            mem_table: BTreeMap::new(),
            ss_table: None,
            block_size,
            compression_type,
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        if let Some(v) = self.mem_table.get(key) {
            if v.is_empty() {
                return None;
            }
            return Some(v.clone());
        }

        self.ss_table.as_ref().and_then(|table| {
            if table.first_key > key || table.last_key < key {
                return None;
            }
            // table.
            let idx = table.find_block_idx(key);
            let block = table.read_block_cached(idx);
            let block_iter = BlockIter::new_seek_to_key(block, key);

            block_iter.next_curr_key().and_then(|k| {
                if k == key {
                    block_iter.next_curr_value()
                } else {
                    None
                }
            })
        })
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

    /// Check if the key exists in the mem table or the sstable
    ///
    /// If the value is empty, it means the key is deleted
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
        Box::new(MemStoreIterator::new(
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
        self.len() == 0
    }

    pub fn size(&self) -> usize {
        self.mem_table
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + v.len())
            + self.ss_table.as_ref().map_or(0, |table| table.data_size())
    }

    pub fn export_all(&mut self) -> Bytes {
        let mut builder = SsTableBuilder::new(self.block_size, self.compression_type);
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
pub struct MemStoreIterator<T, S> {
    mem: T,
    sst: S,
    current_mem: Option<(Bytes, Bytes)>,
    current_sstable: Option<(Bytes, Bytes)>,
    back_mem: Option<(Bytes, Bytes)>,
    back_sstable: Option<(Bytes, Bytes)>,
}

impl<T, S> MemStoreIterator<T, S>
where
    T: DoubleEndedIterator<Item = (Bytes, Bytes)>,
    S: DoubleEndedIterator<Item = (Bytes, Bytes)>,
{
    fn new(mut mem: T, sst: S) -> Self {
        let current_mem = mem.next();
        let back_mem = mem.next_back();
        Self {
            mem,
            sst,
            current_mem,
            back_mem,
            current_sstable: None,
            back_sstable: None,
        }
    }
}

impl<T, S> Iterator for MemStoreIterator<T, S>
where
    T: DoubleEndedIterator<Item = (Bytes, Bytes)>,
    S: DoubleEndedIterator<Item = (Bytes, Bytes)>,
{
    type Item = (Bytes, Bytes);
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_sstable.is_none() {
            if let Some((k, v)) = self.sst.next() {
                self.current_sstable = Some((k, v));
            }
        }

        if self.current_mem.is_none() && self.back_mem.is_some() {
            std::mem::swap(&mut self.back_mem, &mut self.current_mem);
        }
        let ans = match (&self.current_mem, &self.current_sstable) {
            (Some((mem_key, _)), Some((iter_key, _))) => match mem_key.cmp(iter_key) {
                Ordering::Less => self.current_mem.take().map(|kv| {
                    self.current_mem = self.mem.next();
                    kv
                }),
                Ordering::Equal => {
                    self.current_sstable.take();
                    self.current_mem.take().map(|kv| {
                        self.current_mem = self.mem.next();
                        kv
                    })
                }
                Ordering::Greater => self.current_sstable.take(),
            },
            (Some(_), None) => self.current_mem.take().map(|kv| {
                self.current_mem = self.mem.next();
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

impl<T, S> DoubleEndedIterator for MemStoreIterator<T, S>
where
    T: DoubleEndedIterator<Item = (Bytes, Bytes)>,
    S: DoubleEndedIterator<Item = (Bytes, Bytes)>,
{
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.back_sstable.is_none() {
            if let Some((k, v)) = self.sst.next_back() {
                self.back_sstable = Some((k, v));
            }
        }

        if self.back_mem.is_none() && self.current_mem.is_some() {
            std::mem::swap(&mut self.back_mem, &mut self.current_mem);
        }

        let ans = match (&self.back_mem, &self.back_sstable) {
            (Some((mem_key, _)), Some((iter_key, _))) => match mem_key.cmp(iter_key) {
                Ordering::Greater => self.back_mem.take().map(|kv| {
                    self.back_mem = self.mem.next_back();
                    kv
                }),
                Ordering::Equal => {
                    self.back_sstable.take();
                    self.back_mem.take().map(|kv| {
                        self.back_mem = self.mem.next_back();
                        kv
                    })
                }
                Ordering::Less => self.back_sstable.take(),
            },
            (Some(_), None) => self.back_mem.take().map(|kv| {
                self.back_mem = self.mem.next_back();
                kv
            }),
            (None, Some(_)) => self.back_sstable.take(),
            (None, None) => None,
        };
        // TODO: use EmptyFilterIterator
        if let Some((_k, v)) = &ans {
            if v.is_empty() {
                return self.next_back();
            }
        }
        ans
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use crate::MemKvStore;
    use bytes::Bytes;
    #[test]
    fn test_mem_kv_store() {
        let key = &[0];
        let value = Bytes::from_static(&[0]);

        let key2 = &[0, 1];
        let value2 = Bytes::from_static(&[0, 1]);
        let mut store = MemKvStore::default();
        store.set(key, value.clone());
        assert_eq!(store.get(key), Some(value));
        store.remove(key);
        assert!(store.is_empty());
        assert_eq!(store.get(key), None);
        store.compare_and_swap(key, None, value2.clone());
        assert_eq!(store.get(key), Some(value2.clone()));
        assert!(store.contains_key(key));
        assert!(!store.contains_key(key2));

        store.set(key2, value2.clone());
        assert_eq!(store.get(key2), Some(value2.clone()));
        assert_eq!(store.len(), 2);
        assert_eq!(store.size(), 7);
        let bytes = store.export_all();
        let mut new_store = MemKvStore::default();
        assert_eq!(new_store.len(), 0);
        assert_eq!(new_store.size(), 0);
        new_store.import_all(bytes).unwrap();

        let iter1 = store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .collect::<Vec<_>>();
        let iter2 = new_store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .collect::<Vec<_>>();
        assert_eq!(iter1, iter2);

        let iter1 = store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .rev()
            .collect::<Vec<_>>();
        let iter2 = new_store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .rev()
            .collect::<Vec<_>>();
        assert_eq!(iter1, iter2);
    }

    #[test]
    fn test_large_block() {
        let mut store = MemKvStore::default();
        let key = &[0];
        let value = Bytes::from_static(&[0]);

        let key2 = &[0, 1];
        let key3 = &[0, 1, 2];
        let large_value = Bytes::from_iter([0; 1024 * 8]);
        let large_value2 = Bytes::from_iter([0; 1024 * 8]);
        store.set(key, value.clone());
        store.set(key2, large_value.clone());
        let v2 = store.get(&[]);
        assert_eq!(v2, None);
        assert_eq!(store.get(key), Some(value.clone()));
        assert_eq!(store.get(key2), Some(large_value.clone()));
        store.export_all();
        store.set(key3, large_value2.clone());
        assert_eq!(store.get(key3), Some(large_value2.clone()));
        assert_eq!(store.len(), 3);

        let iter = store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .collect::<Vec<_>>();
        assert_eq!(
            iter,
            vec![
                (Bytes::from_static(key), value.clone()),
                (Bytes::from_static(key2), large_value.clone()),
                (Bytes::from_static(key3), large_value2.clone())
            ]
        );

        let iter2 = store
            .scan(
                std::ops::Bound::Included(key),
                std::ops::Bound::Included(key3),
            )
            .collect::<Vec<_>>();
        assert_eq!(iter, iter2);

        let iter3 = store
            .scan(
                std::ops::Bound::Excluded(key),
                std::ops::Bound::Excluded(key3),
            )
            .collect::<Vec<_>>();
        assert_eq!(iter3.len(), 1);
        assert_eq!(iter3[0], (Bytes::from_static(key2), large_value.clone()));

        let v = store.get(key2).unwrap();
        assert_eq!(v, large_value);

        let v2 = store.get(&[]);
        assert_eq!(v2, None);

        store.compare_and_swap(key, Some(value.clone()), large_value.clone());
        assert!(store.contains_key(key));
    }

    #[test]
    fn same_key() {
        let mut store = MemKvStore::default();
        let key = &[0];
        let value = Bytes::from_static(&[0]);
        store.set(key, value.clone());
        store.export_all();
        store.set(key, Bytes::new());
        assert_eq!(store.get(key), None);
        let iter = store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .collect::<Vec<_>>();
        assert_eq!(iter.len(), 0);
        store.set(key, value.clone());
        assert_eq!(store.get(key), Some(value));
    }
}
