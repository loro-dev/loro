use crate::block::BlockIter;
use crate::compress::CompressionType;
use crate::sstable::{SsTable, SsTableBuilder, SsTableIter};
use crate::{KvIterator, MergeIterator};
use bytes::Bytes;

use std::ops::Bound;
use std::{cmp::Ordering, collections::BTreeMap};

#[derive(Debug, Clone)]
pub struct MemKvStore {
    mem_table: BTreeMap<Bytes, Bytes>,
    // From the oldest to the newest
    ss_table: Vec<SsTable>,
    block_size: usize,
    compression_type: CompressionType,
    /// It's only true when using it to fuzz.
    /// Otherwise, importing and exporting GC snapshot relies on this field being false to work.
    should_encode_none: bool,
}

pub struct MemKvConfig {
    block_size: usize,
    compression_type: CompressionType,
    should_encode_none: bool,
}

impl Default for MemKvConfig {
    fn default() -> Self {
        Self {
            block_size: MemKvStore::DEFAULT_BLOCK_SIZE,
            compression_type: CompressionType::LZ4,
            should_encode_none: false,
        }
    }
}

impl MemKvConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn block_size(mut self, block_size: usize) -> Self {
        self.block_size = block_size;
        self
    }

    pub fn compression_type(mut self, compression_type: CompressionType) -> Self {
        self.compression_type = compression_type;
        self
    }

    pub fn should_encode_none(mut self, should_encode_none: bool) -> Self {
        self.should_encode_none = should_encode_none;
        self
    }

    pub fn build(self) -> MemKvStore {
        MemKvStore::new(self)
    }
}

impl MemKvStore {
    pub const DEFAULT_BLOCK_SIZE: usize = 4 * 1024;
    pub fn new(config: MemKvConfig) -> Self {
        Self {
            mem_table: BTreeMap::new(),
            ss_table: Vec::new(),
            block_size: config.block_size,
            compression_type: config.compression_type,
            should_encode_none: config.should_encode_none,
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
            if let Some(k) = block_iter.peek_next_curr_key() {
                let v = block_iter.peek_next_curr_value().unwrap();
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
        if self.mem_table.is_empty() && self.ss_table.len() == 1 {
            return self.ss_table[0].export_all();
        }

        if self.ss_table.len() == 1 {
            return self.export_with_encoded_block();
        }

        let mut builder = SsTableBuilder::new(
            self.block_size,
            self.compression_type,
            self.should_encode_none,
        );
        // we could use scan() here, we should keep the empty value
        let iter = MemStoreIterator::new(
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
        );

        for (k, v) in iter {
            builder.add(k, v);
        }

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

    #[tracing::instrument(level = "debug", skip(self))]
    fn export_with_encoded_block(&mut self) -> Bytes {
        ensure_cov::notify_cov("kv-store::mem_store::export_with_encoded_block");
        let mut mem_iter = self.mem_table.iter().peekable();
        let mut sstable_iter = self.ss_table[0].iter();
        let mut builder = SsTableBuilder::new(
            self.block_size,
            self.compression_type,
            self.should_encode_none,
        );
        'outer: while let Some(next_mem_pair) = mem_iter.peek() {
            let block = loop {
                let Some(block) = sstable_iter.peek_next_block() else {
                    builder.add(next_mem_pair.0.clone(), next_mem_pair.1.clone());
                    mem_iter.next();
                    continue 'outer;
                };
                if block.last_key() < next_mem_pair.0 {
                    builder.add_new_block(block.clone());
                    sstable_iter.next_block();
                    continue;
                }
                break block;
            };

            if block.first_key() > next_mem_pair.0 {
                builder.add(next_mem_pair.0.clone(), next_mem_pair.1.clone());
                mem_iter.next();
                continue;
            }

            // There are overlap between next_mem_pair and block
            let mut iter = BlockIter::new(block.clone());
            let mut next_mem_pair = mem_iter.peek();
            while let Some(k) = iter.peek_next_key() {
                loop {
                    match next_mem_pair {
                        Some(next_mem_pair_inner) => {
                            if k > next_mem_pair_inner.0 {
                                builder.add(
                                    next_mem_pair_inner.0.clone(),
                                    next_mem_pair_inner.1.clone(),
                                );
                                mem_iter.next();
                                next_mem_pair = mem_iter.peek();
                                continue;
                            }
                            if k == next_mem_pair_inner.0 {
                                builder.add(k, next_mem_pair_inner.1.clone());
                                mem_iter.next();
                                next_mem_pair = mem_iter.peek();
                                iter.next();
                                break;
                            }
                            // k < next_mem_pair_inner.0
                            builder.add(k, iter.peek_next_value().unwrap());
                            iter.next();
                            break;
                        }
                        None => {
                            builder.add(k, iter.peek_next_value().unwrap());
                            iter.next();
                            break;
                        }
                    }
                }
            }

            sstable_iter.next_block();
        }

        while let Some(block) = sstable_iter.peek_next_block() {
            builder.add_new_block(block.clone());
            sstable_iter.next_block();
        }

        if builder.is_empty() {
            return Bytes::new();
        }

        drop(mem_iter);
        self.mem_table.clear();
        let ss = builder.build();
        let ans = ss.export_all();
        let _ = std::mem::replace(&mut self.ss_table, vec![ss]);
        ans
    }

    #[allow(unused)]
    fn check_encode_data_correctness(&self, bytes: &Bytes) {
        let this_data: BTreeMap<Bytes, Bytes> =
            self.scan(Bound::Unbounded, Bound::Unbounded).collect();
        let mut other_kv = Self::new(Default::default());
        other_kv.import_all(bytes.clone()).unwrap();
        let other_data: BTreeMap<Bytes, Bytes> =
            other_kv.scan(Bound::Unbounded, Bound::Unbounded).collect();
        assert_eq!(this_data, other_data);
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
        loop {
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
                    Ordering::Less => self.current_mem.take().inspect(|_kv| {
                        self.current_mem = self.mem.next();
                    }),
                    Ordering::Equal => {
                        self.current_sstable.take();
                        self.current_mem.take().inspect(|_kv| {
                            self.current_mem = self.mem.next();
                        })
                    }
                    Ordering::Greater => self.current_sstable.take(),
                },
                (Some(_), None) => self.current_mem.take().inspect(|_kv| {
                    self.current_mem = self.mem.next();
                }),
                (None, Some(_)) => self.current_sstable.take(),
                (None, None) => None,
            };

            if self.filter_empty {
                if let Some((_k, v)) = &ans {
                    if v.is_empty() {
                        continue;
                    }
                }
            }

            return ans;
        }
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
                Ordering::Greater => self.back_mem.take().inspect(|_kv| {
                    self.back_mem = self.mem.next_back();
                }),
                Ordering::Equal => {
                    self.back_sstable.take();
                    self.back_mem.take().inspect(|_kv| {
                        self.back_mem = self.mem.next_back();
                    })
                }
                Ordering::Less => self.back_sstable.take(),
            },
            (Some(_), None) => self.back_mem.take().inspect(|_kv| {
                self.back_mem = self.mem.next_back();
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

    use crate::{mem_store::MemKvConfig, MemKvStore};
    use bytes::Bytes;
    #[test]
    fn test_mem_kv_store() {
        let key = &[0];
        let value = Bytes::from_static(&[0]);

        let key2 = &[0, 1];
        let value2 = Bytes::from_static(&[0, 1]);
        let mut store = new_store();
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
        let mut new_store = new_store();
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
        let mut store = new_store();
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
        let mut store = new_store();
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
        dev_utils::setup_test_log();
        let a = Bytes::from_static(b"a");
        let b = Bytes::from_static(b"b");
        let c = Bytes::from_static(b"c");
        let d = Bytes::from_static(b"d");
        let e = Bytes::from_static(b"e");
        let mut store = new_store();
        store.set(&a, a.clone());
        store.export_all();
        store.set(&c, c.clone());
        let encode1 = store.export_all();
        let mut store2 = new_store();
        store2.set(&b, b.clone());
        store2.export_all();
        store2.set(&c, Bytes::new());
        let encode2 = store2.export_all();
        let mut store3 = new_store();
        store3.set(&d, d.clone());
        store3.set(&a, Bytes::new());
        tracing::info_span!("export da").in_scope(|| {
            store3.export_all();
        });
        store3.set(&e, e.clone());
        store3.set(&c, c.clone());
        let encode3 = tracing::info_span!("export ec").in_scope(|| store3.export_all());

        let mut store = new_store();
        store.import_all(encode1).unwrap();
        store.import_all(encode2).unwrap();
        store.import_all(encode3).unwrap();
        assert_eq!(store.get(&a), None);
        assert_eq!(store.get(&b), Some(b.clone()));
        assert_eq!(store.get(&c), Some(c.clone()));
        assert_eq!(store.get(&d), Some(d.clone()));
        assert_eq!(store.get(&e), Some(e.clone()));
    }

    fn new_store() -> MemKvStore {
        MemKvStore::new(MemKvConfig::default().should_encode_none(true))
    }
}
