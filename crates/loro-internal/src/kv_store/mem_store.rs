//! # MemKvStore Documentation
//!
//! MemKvStore use SSTable as backend. The SSTable (Sorted String Table) is a persistent data structure used for storing key-value pairs in a sorted manner. This document describes the binary format of the SSTable.
//!
//! ## Overall Structure
//!
//! The SSTable consists of the following sections:
//!
//! ```
//! ┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
//! │ MemKVStore                                                                                      │
//! │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┐│
//! │  Magic Number │ Schema Version │ Block Chunk   ...  │  Block Chunk    Block Meta │ Meta Offset  │
//! ││     u32      │       u8       │    bytes    │      │     bytes     │   bytes    │     u32     ││
//! │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ │
//! └─────────────────────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! 1. Magic Number (4 bytes): A fixed value "LORO" to identify the file format.
//! 2. Schema Version (1 byte): The version of the MemKVStore schema.
//! 3. Block Chunks: A series of data blocks containing key-value pairs.
//! 4. Block Meta: Metadata for all blocks, including block offset, first key, is_large flag, and last key if not large.
//! 5. Meta Offset (4 bytes): The offset of the Block Meta section from the beginning of the file.
//!
//! ## Block Types
//!
//! There are two types of blocks: Normal Blocks and Large Value Blocks.
//!
//! ### Normal Block
//!
//! Normal blocks store multiple key-value pairs with compressed keys.
//!
//! ```
//! ┌────────────────────────────────────────────────────────────────────────────────────────────┐
//! │Block                                                                                   │
//! │┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ┬ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ │
//! │ Key Value Chunk  ...  │Key Value Chunk  offset │ ...  │ offset  kv len │Block Checksum││
//! ││     bytes     │      │     bytes     │  u16   │      │  u16  │  u16   │     u32       │
//! │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ┘│
//! └────────────────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Each Key Value Chunk is encoded as follows:
//!
//! ```
//! ┌───────────────────────────────────────────────────────────────┐
//! │  Key Value Chunk                                              │
//! │┌ ─ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ┬ ─ ─ ─ ┐│
//! │ common prefix len key suffix len│key suffix│value len  value  │
//! ││       u8        │     u16      │  bytes   │   u16   │ bytes ││
//! │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ │
//! └───────────────────────────────────────────────────────────────┘
//! ```
//!
//! Encoding:
//! 1. Compress key-value pairs data as Key Value Chunk.
//! 2. Write offsets for each key-value pair.
//! 3. Write the number of key-value pairs.
//! 4. **Compress** the entire block using LZ4.
//! 5. Calculate and append CRC32 checksum.
//!
//! Decoding:
//! 1. Verify the CRC32 checksum.
//! 2. **Decompress** the block using LZ4.
//! 3. Read the number of key-value pairs.
//! 4. Read offsets for each key-value pair.
//! 5. Parse individual key-value chunks.
//!
//! ### Large Value Block
//!
//! Large Value Blocks store a single key-value pair with a large value.
//!
//! ```
//! ┌──────────────────────────┐
//! │Large Block               │
//! │┌ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ │
//! │  value   Block Checksum ││
//! ││ bytes │      u32        │
//! │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘│
//! └──────────────────────────┘
//! ```
//!
//! Encoding:
//! 1. Write the value bytes.
//! 2. Calculate and append CRC32 checksum.
//!
//! Decoding:
//! 1. Verify the CRC32 checksum.
//! 2. Read the value bytes.
//!
//! We need not encode the length of value, because we can get the whole Block by offset in meta.
//!
//! ## Block Meta
//!
//! The Block Meta section contains metadata for all blocks in the SSTable.
//!
//! ```
//! ┌────────────────────────────────────────────────────────────┐
//! │ All Block Meta                                             │
//! │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ │
//! │  block length │ Block Meta │ ...  │ Block Meta │ checksum ││
//! ││     u32      │   bytes    │      │   bytes    │   u32     │
//! │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘│
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! Each Block Meta entry is encoded as follows:
//!
//! ```
//! ┌──────────────────────────────────────────────────────────────────────────────────────┐
//! │ Block Meta                                                                           │
//! │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ┐ │
//! │  block offset │ first key len   first key   is large │ last key len     last key     │
//! ││     u32      │      u16      │   bytes   │    u8    │  u16(option)  │bytes(option)│ │
//! │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
//! └──────────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Encoding:
//! 1. Write the number of blocks.
//! 2. For each block, write its metadata (offset, first key, is_large flag, and last key if not large).
//! 3. Calculate and append CRC32 checksum.
//!
//! Decoding:
//! 1. Read the number of blocks.
//! 2. For each block, read its metadata.
//! 3. Verify the CRC32 checksum.
//!
use fxhash::FxHashSet;
use iter::BlockIter;
use sstable::{SsTable, SsTableBuilder, SsTableIter};

use super::*;
use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

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
}

impl KvStore for MemKvStore {
    fn get(&self, key: &[u8]) -> Option<Bytes> {
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

    fn set(&mut self, key: &[u8], value: Bytes) {
        self.mem_table.insert(Bytes::copy_from_slice(key), value);
    }

    fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool {
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

    fn remove(&mut self, key: &[u8]) {
        self.set(key, Bytes::new());
    }

    fn contains_key(&self, key: &[u8]) -> bool {
        if self.mem_table.contains_key(key) {
            return !self.mem_table.get(key).unwrap().is_empty();
        }
        if let Some(table) = &self.ss_table {
            return table.contains_key(key);
        }
        false
    }

    fn scan(
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

    fn len(&self) -> usize {
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

    fn size(&self) -> usize {
        self.mem_table
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + v.len())
            + self.ss_table.as_ref().map_or(0, |table| table.data_size())
    }

    fn export_all(&mut self) -> Bytes {
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

    fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
        if bytes.is_empty() {
            self.ss_table = None;
            return Ok(());
        }
        let ss_table = SsTable::import_all(bytes).map_err(|e| e.to_string())?;
        self.ss_table = Some(ss_table);
        Ok(())
    }

    fn clone_store(&self) -> Arc<std::sync::Mutex<dyn KvStore>> {
        Arc::new(std::sync::Mutex::new(self.clone()))
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
