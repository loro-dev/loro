use crate::block::BlockIter;
use crate::compress::CompressionType;
use crate::sstable::{SsTable, SsTableBuilder, SsTableIter};
use crate::MergeIterator;
use bytes::Bytes;

use std::ops::Bound;
use std::{cmp::Ordering, collections::BTreeMap};

const DEFAULT_BLOCK_SIZE: usize = 4 * 1024;

#[derive(Debug, Clone)]
pub struct MemKvStore {
    mem_table: BTreeMap<Bytes, Bytes>,
    // From the oldest to the newest
    ss_table: Vec<SsTable>,
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
            ss_table: Vec::new(),
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

        for table in self.ss_table.iter().rev() {
            if table.first_key > key || table.last_key < key {
                continue;
            }
            // table.
            let idx = table.find_block_idx(key);
            let block = table.read_block_cached(idx);
            let block_iter = BlockIter::new_seek_to_key(block, key);
            if let Some(k) = block_iter.next_curr_key() {
                let v = block_iter.next_curr_value().unwrap();
                if k == key {
                    return if v.is_empty() { None } else { Some(v) };
                }
            }
        }
        None
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

        for table in self.ss_table.iter().rev() {
            if table.contains_key(key) {
                if let Some(v) = table.get(key) {
                    return !v.is_empty();
                }
            }
        }
        false
    }

    pub fn scan(
        &self,
        start: std::ops::Bound<&[u8]>,
        end: std::ops::Bound<&[u8]>,
    ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_> {
        if self.ss_table.is_empty() {
            return Box::new(
                self.mem_table
                    .range::<[u8], _>((start, end))
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }

        Box::new(MemStoreIterator::new(
            self.mem_table
                .range::<[u8], _>((start, end))
                .map(|(k, v)| (k.clone(), v.clone())),
            MergeIterator::new(
                self.ss_table
                    .iter()
                    .rev()
                    .map(|table| SsTableIter::new_scan(table, start, end))
                    .collect(),
            ),
            true,
        ))
    }

    /// The number of valid keys in the mem table and sstable, it's expensive to call
    pub fn len(&self) -> usize {
        // TODO: PERF
        self.scan(Bound::Unbounded, Bound::Unbounded).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn size(&self) -> usize {
        self.mem_table
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + v.len())
            + self
                .ss_table
                .iter()
                .map(|table| table.data_size())
                .sum::<usize>()
    }

    pub fn export_all(&mut self) -> Bytes {
        let mut builder = SsTableBuilder::new(self.block_size, self.compression_type);
        // we could use scan() here, we should keep the empty value
        for (k, v) in MemStoreIterator::new(
            self.mem_table
                .range::<[u8], _>((Bound::Unbounded, Bound::Unbounded))
                .map(|(k, v)| (k.clone(), v.clone())),
            MergeIterator::new(
                self.ss_table
                    .iter()
                    .rev()
                    .map(|table| SsTableIter::new_scan(table, Bound::Unbounded, Bound::Unbounded))
                    .collect(),
            ),
            false,
        ) {
            builder.add(k, v);
        }

        builder.finish_block();
        if builder.is_empty() {
            return Bytes::new();
        }
        self.mem_table.clear();
        let ss = builder.build();
        let ans = ss.export_all();
        let _ = std::mem::replace(&mut self.ss_table, vec![ss]);
        ans
    }

    /// We can import several times, the latter will override the former.
    pub fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        let ss_table = SsTable::import_all(bytes).map_err(|e| e.to_string())?;
        self.ss_table.push(ss_table);
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
    filter_empty: bool,
}

impl<T, S> MemStoreIterator<T, S>
where
    T: DoubleEndedIterator<Item = (Bytes, Bytes)>,
    S: DoubleEndedIterator<Item = (Bytes, Bytes)>,
{
    fn new(mut mem: T, sst: S, filter_empty: bool) -> Self {
        let current_mem = mem.next();
        let back_mem = mem.next_back();
        Self {
            mem,
            sst,
            current_mem,
            back_mem,
            current_sstable: None,
            back_sstable: None,
            filter_empty,
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

        if self.filter_empty {
            if let Some((_k, v)) = &ans {
                if v.is_empty() {
                    return self.next();
                }
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
        if self.filter_empty {
            if let Some((_k, v)) = &ans {
                if v.is_empty() {
                    return self.next_back();
                }
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

    #[test]
    fn import_several_times() {
        let a = Bytes::from_static(b"a");
        let b = Bytes::from_static(b"b");
        let c = Bytes::from_static(b"c");
        let d = Bytes::from_static(b"d");
        let e = Bytes::from_static(b"e");
        let mut store = MemKvStore::default();
        store.set(&a, a.clone());
        store.export_all();
        store.set(&c, c.clone());
        let encode1 = store.export_all();
        let mut store2 = MemKvStore::default();
        store2.set(&b, b.clone());
        store2.export_all();
        store2.set(&c, Bytes::new());
        let encode2 = store2.export_all();
        let mut store3 = MemKvStore::default();
        store3.set(&d, d.clone());
        store3.set(&a, Bytes::new());
        store3.export_all();
        store3.set(&e, e.clone());
        store3.set(&c, c.clone());
        let encode3 = store3.export_all();

        let mut store = MemKvStore::default();
        store.import_all(encode1).unwrap();
        store.import_all(encode2).unwrap();
        store.import_all(encode3).unwrap();
        assert_eq!(store.get(&a), None);
        assert_eq!(store.get(&b), Some(b.clone()));
        assert_eq!(store.get(&c), Some(c.clone()));
        assert_eq!(store.get(&d), Some(d.clone()));
        assert_eq!(store.get(&e), Some(e.clone()));
    }
}
