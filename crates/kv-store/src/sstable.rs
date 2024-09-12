use std::{fmt::Debug, ops::Bound, sync::Arc};

use bytes::{Buf, BufMut, Bytes};
use loro_common::{LoroError, LoroResult};

use super::block::BlockIter;
use crate::{
    block::{Block, BlockBuilder},
    compress::CompressionType,
    iter::KvIterator,
    utils::{get_u16_le, get_u32_le, get_u8_le},
};

pub(crate) const XXH_SEED: u32 = u32::from_le_bytes(*b"LORO");
const MAGIC_BYTES: [u8; 4] = *b"LORO";
const CURRENT_SCHEMA_VERSION: u8 = 0;
pub const SIZE_OF_U8: usize = std::mem::size_of::<u8>();
pub const SIZE_OF_U16: usize = std::mem::size_of::<u16>();
pub const SIZE_OF_U32: usize = std::mem::size_of::<u32>();
// TODO: cache size
const DEFAULT_CACHE_SIZE: usize = 1 << 20;
const MAX_BLOCK_NUM: u32 = 10_000_000;

/// ┌──────────────────────────────────────────────────────────────────────────────────────────┐
/// │ Block Meta                                                                               │
/// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ┐ │
/// │  block offset │ first key len   first key    block type  │ last key len     last key     │
/// ││     u32      │      u16      │   bytes   │      u8      │  u16(option)  │bytes(option)│ │
/// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
/// └──────────────────────────────────────────────────────────────────────────────────────────┘
#[derive(Debug, Clone)]
pub(crate) struct BlockMeta {
    offset: usize,
    is_large: bool,
    compression_type: CompressionType,
    first_key: Bytes,
    last_key: Option<Bytes>,
}

impl BlockMeta {
    /// ┌────────────────────────────────────────────────────────────┐
    /// │ All Block Meta                                             │
    /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ │
    /// │  block length │ Block Meta │ ...  │ Block Meta │ checksum ││
    /// ││     u32      │   bytes    │      │   bytes    │   u32     │
    /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘│
    /// └────────────────────────────────────────────────────────────┘
    fn encode_meta(meta: &[BlockMeta], buf: &mut Vec<u8>) {
        // the number of blocks
        let mut estimated_size = SIZE_OF_U32;
        for m in meta {
            // offset
            estimated_size += SIZE_OF_U32;
            // first key length
            estimated_size += SIZE_OF_U16;
            // first key
            estimated_size += m.first_key.len();
            // is large
            estimated_size += SIZE_OF_U8;
            if m.is_large {
                continue;
            }
            // last key length
            estimated_size += SIZE_OF_U16;
            // last key
            estimated_size += m.last_key.as_ref().unwrap().len();
        }
        // checksum
        estimated_size += SIZE_OF_U32;

        buf.reserve(estimated_size);
        let ori_length = buf.len();
        buf.put_u32_le(meta.len() as u32);
        for m in meta {
            buf.put_u32_le(m.offset as u32);
            buf.put_u16_le(m.first_key.len() as u16);
            buf.put_slice(&m.first_key);
            let large_and_compress = (m.is_large as u8) << 7 | m.compression_type as u8;
            buf.put_u8(large_and_compress);
            if m.is_large {
                continue;
            }
            buf.put_u16_le(m.last_key.as_ref().unwrap().len() as u16);
            buf.put_slice(m.last_key.as_ref().unwrap());
        }
        let checksum = xxhash_rust::xxh32::xxh32(&buf[ori_length + 4..], XXH_SEED);
        buf.put_u32_le(checksum);
    }

    fn decode_meta(data: &[u8]) -> LoroResult<Vec<BlockMeta>> {
        let (num, mut data) = get_u32_le(data)?;
        if num > MAX_BLOCK_NUM {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let mut ans = Vec::with_capacity(num as usize);
        if data.len() < SIZE_OF_U32 {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let checksum = xxhash_rust::xxh32::xxh32(&data[..data.len() - SIZE_OF_U32], XXH_SEED);
        for _ in 0..num {
            let (offset, buf) = get_u32_le(data)?;
            let (first_key_len, mut buf) = get_u16_le(buf)?;
            if buf.len() < first_key_len as usize {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            let first_key = buf.copy_to_bytes(first_key_len as usize);
            let (is_large_and_compression_type, buf) = get_u8_le(buf)?;
            let is_large = is_large_and_compression_type & 0b1000_0000 != 0;
            let compression_type = is_large_and_compression_type & 0b0111_1111;
            if is_large {
                ans.push(BlockMeta {
                    offset: offset as usize,
                    is_large,
                    compression_type: compression_type.try_into()?,
                    first_key,
                    last_key: None,
                });
                data = buf;
                continue;
            }
            let (last_key_len, mut buf) = get_u16_le(buf)?;
            if buf.len() < last_key_len as usize {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            let last_key = buf.copy_to_bytes(last_key_len as usize);
            ans.push(BlockMeta {
                offset: offset as usize,
                is_large,
                compression_type: compression_type.try_into()?,
                first_key,
                last_key: Some(last_key),
            });
            data = buf;
        }
        let (checksum_read, _) = get_u32_le(data)?;
        if checksum != checksum_read {
            return Err(LoroError::DecodeChecksumMismatchError);
        }
        Ok(ans)
    }
}

pub(crate) struct SsTableBuilder {
    block_builder: BlockBuilder,
    first_key: Bytes,
    last_key: Bytes,
    data: Vec<u8>,
    meta: Vec<BlockMeta>,
    block_size: usize,
    compression_type: CompressionType,
    // TODO: bloom filter
}

impl SsTableBuilder {
    pub fn new(block_size: usize, compression_type: CompressionType) -> Self {
        let mut data = Vec::with_capacity(5);
        data.put_u32_le(u32::from_le_bytes(MAGIC_BYTES));
        data.put_u8(CURRENT_SCHEMA_VERSION);
        Self {
            block_builder: BlockBuilder::new(block_size),
            first_key: Bytes::new(),
            last_key: Bytes::new(),
            data,
            meta: Vec::new(),
            block_size,
            compression_type,
        }
    }

    pub fn add(&mut self, key: Bytes, value: Bytes) {
        if self.first_key.is_empty() {
            self.first_key = key.clone();
        }
        if self.block_builder.add(&key, &value) {
            self.last_key = key;
            return;
        }

        self.finish_block();

        assert!(self.block_builder.add(&key, &value));
        self.first_key = key.clone();
        self.last_key = key;
    }

    pub fn is_empty(&self) -> bool {
        self.meta.is_empty()
    }

    pub(crate) fn finish_block(&mut self) {
        if self.block_builder.is_empty() {
            return;
        }
        let builder =
            std::mem::replace(&mut self.block_builder, BlockBuilder::new(self.block_size));
        let block = builder.build();
        let offset = self.data.len();
        let real_compression_type = block.encode(&mut self.data, self.compression_type);
        let is_large = block.is_large();
        let meta = BlockMeta {
            offset,
            is_large,
            compression_type: real_compression_type,
            first_key: std::mem::take(&mut self.first_key),
            last_key: if is_large {
                None
            } else {
                Some(std::mem::take(&mut self.last_key))
            },
        };
        self.meta.push(meta);
    }

    /// ┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
    /// │ SsTable                                                                                         │
    /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┐│
    /// │  Magic Number │ Schema Version │ Block Chunk   ...  │  Block Chunk    Block Meta │ meta offset  │
    /// ││     u32      │       u8       │    bytes    │      │     bytes     │   bytes    │     u32     ││
    /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ │
    /// └─────────────────────────────────────────────────────────────────────────────────────────────────┘
    pub fn build(mut self) -> SsTable {
        self.finish_block();
        let mut buf = self.data;
        let meta_offset = buf.len() as u32;
        BlockMeta::encode_meta(&self.meta, &mut buf);
        buf.put_u32_le(meta_offset);
        let first_key = self
            .meta
            .first()
            .map(|m| m.first_key.clone())
            .unwrap_or_default();
        let last_key = self
            .meta
            .last()
            .map(|m| {
                m.last_key.clone().unwrap_or(
                    self.meta
                        .last()
                        .map(|m| m.first_key.clone())
                        .unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        SsTable {
            data: Bytes::from(buf),
            first_key,
            last_key,
            meta: self.meta,
            meta_offset: meta_offset as usize,
            block_cache: BlockCache::new(DEFAULT_CACHE_SIZE),
        }
    }
}

type BlockCache = quick_cache::sync::Cache<usize, Arc<Block>>;

#[derive(Debug)]
pub struct SsTable {
    // TODO: mmap?
    data: Bytes,
    pub(crate) first_key: Bytes,
    pub(crate) last_key: Bytes,
    meta: Vec<BlockMeta>,
    meta_offset: usize,
    block_cache: BlockCache,
}

impl Clone for SsTable {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            first_key: self.first_key.clone(),
            last_key: self.last_key.clone(),
            meta: self.meta.clone(),
            meta_offset: self.meta_offset,
            block_cache: BlockCache::new(DEFAULT_CACHE_SIZE),
        }
    }
}

impl SsTable {
    pub fn export_all(&self) -> Bytes {
        self.data.clone()
    }

    pub fn iter(&self) -> SsTableIter {
        SsTableIter::new(self)
    }

    ///
    ///
    /// # Errors
    /// - [LoroError::DecodeChecksumMismatchError]
    /// - [LoroError::DecodeError]
    ///    - "Invalid magic number"
    ///    - "Invalid schema version"
    pub fn import_all(bytes: Bytes) -> LoroResult<Self> {
        // magic number + schema version + meta offset
        if bytes.len() < SIZE_OF_U32 + SIZE_OF_U8 + SIZE_OF_U32 {
            return Err(LoroError::DecodeError("Invalid sstable bytes".into()));
        }
        let magic_number = u32::from_le_bytes((&bytes[..SIZE_OF_U32]).try_into().unwrap());
        if magic_number != u32::from_le_bytes(MAGIC_BYTES) {
            return Err(LoroError::DecodeError("Invalid magic number".into()));
        }
        let schema_version = bytes[SIZE_OF_U32];
        match schema_version {
            CURRENT_SCHEMA_VERSION => {}
            _ => {
                return Err(LoroError::DecodeError(
                    format!(
                        "Invalid schema version {}, 
            current support max version is {}",
                        schema_version, CURRENT_SCHEMA_VERSION
                    )
                    .into(),
                ))
            }
        }
        let data_len = bytes.len();
        let meta_offset = (&bytes[data_len - SIZE_OF_U32..]).get_u32_le() as usize;
        if meta_offset >= data_len - SIZE_OF_U32 {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let raw_meta = &bytes[meta_offset..data_len - SIZE_OF_U32];
        let meta = BlockMeta::decode_meta(raw_meta)?;
        Self::check_block_checksum(&meta, &bytes, meta_offset)?;
        let first_key = meta
            .first()
            .map(|m| m.first_key.clone())
            .unwrap_or_default();
        let last_key = meta
            .last()
            .map(|m| {
                m.last_key
                    .clone()
                    .unwrap_or(meta.last().map(|m| m.first_key.clone()).unwrap_or_default())
            })
            .unwrap_or_default();
        let ans = Self {
            data: bytes,
            first_key,
            last_key,
            meta,
            meta_offset,
            block_cache: BlockCache::new(DEFAULT_CACHE_SIZE),
        };
        Ok(ans)
    }

    fn check_block_checksum(
        meta: &[BlockMeta],
        bytes: &Bytes,
        meta_offset: usize,
    ) -> LoroResult<()> {
        for i in 0..meta.len() {
            let offset = meta[i].offset;
            let offset_end = meta.get(i + 1).map_or(meta_offset, |m| m.offset);
            if offset_end > bytes.len() {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            let raw_block_and_check = bytes.slice(offset..offset_end);
            let checksum = raw_block_and_check
                .slice(raw_block_and_check.len() - SIZE_OF_U32..)
                .get_u32_le();
            if checksum
                != xxhash_rust::xxh32::xxh32(
                    &raw_block_and_check[..raw_block_and_check.len() - SIZE_OF_U32],
                    XXH_SEED,
                )
            {
                return Err(LoroError::DecodeChecksumMismatchError);
            }
        }
        Ok(())
    }

    pub fn find_block_idx(&self, key: &[u8]) -> usize {
        self.meta
            .partition_point(|meta| meta.first_key <= key)
            .saturating_sub(1)
    }

    pub fn find_back_block_idx(&self, key: &[u8]) -> usize {
        self.meta
            .partition_point(|meta| meta.last_key.as_ref().unwrap_or(&meta.first_key) <= key)
            .min(self.meta.len() - 1)
    }

    fn read_block(&self, block_idx: usize) -> Arc<Block> {
        let offset = self.meta[block_idx].offset;
        let offset_end = self
            .meta
            .get(block_idx + 1)
            .map_or(self.meta_offset, |m| m.offset);
        let raw_block_and_check = self.data.slice(offset..offset_end);
        Arc::new(Block::decode(
            raw_block_and_check,
            self.meta[block_idx].is_large,
            self.meta[block_idx].first_key.clone(),
            self.meta[block_idx].compression_type,
        ))
    }

    pub(crate) fn read_block_cached(&self, block_idx: usize) -> Arc<Block> {
        self.block_cache
            .get_or_insert_with(&block_idx, || {
                Ok::<_, LoroError>(self.read_block(block_idx))
            })
            .unwrap()
    }

    pub fn contains_key(&self, key: &[u8]) -> bool {
        if self.first_key > key || self.last_key < key {
            return false;
        }
        let idx = self.find_block_idx(key);
        let block = self.read_block_cached(idx);
        let block_iter = BlockIter::new_seek_to_key(block, key);
        block_iter.peek_next_curr_key() == Some(Bytes::copy_from_slice(key))
    }

    #[allow(unused)]
    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        if self.first_key > key || self.last_key < key {
            return None;
        }
        let idx = self.find_block_idx(key);
        let block = self.read_block_cached(idx);
        let block_iter = BlockIter::new_seek_to_key(block, key);
        block_iter.peek_next_curr_key().and_then(|k| {
            if k == key {
                block_iter.peek_next_curr_value()
            } else {
                None
            }
        })
    }

    pub fn data_size(&self) -> usize {
        self.data.len()
    }

    pub fn meta_len(&self) -> usize {
        self.meta.len()
    }
}

#[derive(Clone)]
pub struct SsTableIter<'a> {
    table: &'a SsTable,
    iter: SsTableIterInner,
    next_block_idx: usize,
    back_block_idx: isize,
}

impl<'a> Debug for SsTableIter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsTableIter")
            .field("iter", &self.iter)
            .field("next_block_idx", &self.next_block_idx)
            .field("back_block_idx", &self.back_block_idx)
            .finish()
    }
}

impl<'a> SsTableIter<'a> {
    fn new(table: &'a SsTable) -> Self {
        Self::new_scan(table, Bound::Unbounded, Bound::Unbounded)
    }

    pub fn new_scan(table: &'a SsTable, start: Bound<&[u8]>, end: Bound<&[u8]>) -> Self {
        let (table_idx, mut iter, excluded) = match start {
            Bound::Included(start) => {
                let idx = table.find_block_idx(start);
                let block = table.read_block_cached(idx);
                let iter = BlockIter::new_seek_to_key(block, start);
                (idx, iter, None)
            }
            Bound::Excluded(start) => {
                let idx = table.find_block_idx(start);
                let block = table.read_block_cached(idx);
                let iter = BlockIter::new_seek_to_key(block, start);
                (idx, iter, Some(start))
            }
            Bound::Unbounded => {
                let block = table.read_block_cached(0);
                let iter = BlockIter::new(block);
                (0, iter, None)
            }
        };
        let (end_idx, end_iter, end_excluded) = match end {
            Bound::Included(end) => {
                let end_idx = table.find_back_block_idx(end);
                if end_idx == table_idx {
                    iter.back_to_key(end);
                    // if the next back is invalid, the next should also be invalid
                    if !iter.has_next_back() {
                        iter.next();
                    }
                    (end_idx, None, None)
                } else {
                    let block = table.read_block_cached(end_idx);
                    let iter = BlockIter::new_back_to_key(block, end);
                    (end_idx, Some(iter), None)
                }
            }
            Bound::Excluded(end) => {
                let end_idx = table.find_back_block_idx(end);
                if end_idx == table_idx {
                    iter.back_to_key(end);
                    // if the next back is invalid, the next should also be invalid
                    if !iter.has_next_back() {
                        iter.next();
                    }
                    (end_idx, None, Some(end))
                } else {
                    let block = table.read_block_cached(end_idx);
                    let iter = BlockIter::new_back_to_key(block, end);
                    (end_idx, Some(iter), Some(end))
                }
            }
            Bound::Unbounded => {
                let end_idx = table.meta.len() - 1;
                if end_idx == table_idx {
                    (end_idx, None, None)
                } else {
                    let block = table.read_block_cached(end_idx);
                    let iter = BlockIter::new(block);
                    (end_idx, Some(iter), None)
                }
            }
        };

        let mut ans = if let Some(end_iter) = end_iter {
            debug_assert!(end_idx > table_idx);
            SsTableIter {
                table,
                iter: SsTableIterInner::Double {
                    front: iter,
                    back: end_iter,
                },
                next_block_idx: table_idx,
                back_block_idx: end_idx as isize,
            }
        } else {
            debug_assert!(end_idx == table_idx);
            SsTableIter {
                table,
                iter: SsTableIterInner::Same(iter),
                next_block_idx: table_idx,
                back_block_idx: end_idx as isize,
            }
        };
        // the current iter may be empty, but has next iter. we need to skip the empty iter
        ans.skip_next_empty();
        ans.skip_next_back_empty();

        if let Some(key) = excluded {
            if ans.has_next() && ans.peek_next_key().unwrap() == key {
                ans.next();
            }
        }
        if let Some(key) = end_excluded {
            if ans.has_next_back() && ans.peek_next_back_key().unwrap() == key {
                ans.next_back();
            }
        }
        ans
    }

    fn skip_next_empty(&mut self) {
        while self.has_next() && !self.iter.front_iter().has_next() {
            self.next();
        }
    }

    fn skip_next_back_empty(&mut self) {
        while self.has_next_back() && !self.iter.back_iter().has_next_back() {
            self.next_back();
        }
    }

    fn has_next(&self) -> bool {
        self.iter.front_iter().has_next() || (self.next_block_idx as isize) < self.back_block_idx
    }

    pub fn peek_next_key(&self) -> Option<Bytes> {
        if self.has_next() {
            self.iter.front_iter().peek_next_curr_key()
        } else {
            None
        }
    }

    pub fn peek_next_value(&self) -> Option<Bytes> {
        if self.has_next() {
            self.iter.front_iter().peek_next_curr_value()
        } else {
            None
        }
    }

    fn has_next_back(&self) -> bool {
        self.iter.back_iter().has_next_back()
            || (self.next_block_idx as isize) < self.back_block_idx
    }

    pub fn peek_next_back_key(&self) -> Option<Bytes> {
        if !self.has_next_back() {
            return None;
        }
        self.iter.back_iter().peek_back_curr_key()
    }

    pub fn peek_next_back_value(&self) -> Option<Bytes> {
        if !self.has_next_back() {
            return None;
        }
        self.iter.back_iter().peek_back_curr_value()
    }

    pub fn next(&mut self) {
        self.iter.front_iter_mut().next();
        if !self.iter.front_iter().has_next() {
            self.next_block_idx += 1;
            if self.next_block_idx > self.back_block_idx as usize {
                return;
            }
            if self.next_block_idx == self.back_block_idx as usize && !self.iter.is_same() {
                self.iter.convert_back_as_same();
            } else if self.next_block_idx < self.table.meta.len() {
                let block = self.table.read_block_cached(self.next_block_idx);
                self.iter.reset_front(BlockIter::new(block));
                self.skip_next_empty();
            }
        }
    }

    pub fn next_back(&mut self) {
        let iter = self.iter.back_iter_mut();
        iter.next_back();
        if !iter.has_next_back() {
            self.back_block_idx -= 1;
            if self.next_block_idx > self.back_block_idx as usize {
                return;
            }
            if self.next_block_idx == self.back_block_idx as usize && !self.iter.is_same() {
                self.iter.convert_front_as_same();
            } else if self.back_block_idx > 0 {
                let block = self.table.read_block_cached(self.back_block_idx as usize);
                self.iter.reset_back(BlockIter::new(block));
                self.skip_next_back_empty();
            }
        }
    }
}

impl<'a> KvIterator for SsTableIter<'a> {
    fn peek_next_key(&self) -> Option<Bytes> {
        self.peek_next_key()
    }

    fn peek_next_value(&self) -> Option<Bytes> {
        self.peek_next_value()
    }

    fn next_(&mut self) {
        self.next()
    }

    fn has_next(&self) -> bool {
        self.has_next()
    }

    fn peek_next_back_key(&self) -> Option<Bytes> {
        self.peek_next_back_key()
    }

    fn peek_next_back_value(&self) -> Option<Bytes> {
        self.peek_next_back_value()
    }

    fn next_back_(&mut self) {
        self.next_back()
    }

    fn has_next_back(&self) -> bool {
        self.has_next_back()
    }
}

impl<'a> Iterator for SsTableIter<'a> {
    type Item = (Bytes, Bytes);
    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next() {
            return None;
        }
        let key = self.peek_next_key().unwrap();
        let value = self.peek_next_value().unwrap();
        self.next();
        Some((key, value))
    }
}

impl<'a> DoubleEndedIterator for SsTableIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if !self.has_next_back() {
            return None;
        }
        let key = self.peek_next_back_key().unwrap();
        let value = self.peek_next_back_value().unwrap();
        self.next_back();
        Some((key, value))
    }
}

pub(crate) fn get_common_prefix_len_and_strip<'a, T: AsRef<[u8]> + ?Sized>(
    this: &'a T,
    last: &T,
) -> (u8, &'a [u8]) {
    let mut common_prefix_len = 0;
    for (i, (a, b)) in this.as_ref().iter().zip(last.as_ref().iter()).enumerate() {
        if a != b || i == 255 {
            common_prefix_len = i;
            break;
        }
    }

    let suffix = &this.as_ref()[common_prefix_len..];
    (common_prefix_len as u8, suffix)
}

#[derive(Clone, Debug)]
enum SsTableIterInner {
    Same(BlockIter),
    Double { front: BlockIter, back: BlockIter },
}

impl SsTableIterInner {
    fn front_iter(&self) -> &BlockIter {
        match self {
            SsTableIterInner::Same(iter) => iter,
            SsTableIterInner::Double { front, .. } => front,
        }
    }

    fn front_iter_mut(&mut self) -> &mut BlockIter {
        match self {
            SsTableIterInner::Same(iter) => iter,
            SsTableIterInner::Double { front, .. } => front,
        }
    }

    fn back_iter(&self) -> &BlockIter {
        match self {
            SsTableIterInner::Same(iter) => iter,
            SsTableIterInner::Double { back, .. } => back,
        }
    }

    fn back_iter_mut(&mut self) -> &mut BlockIter {
        match self {
            SsTableIterInner::Same(iter) => iter,
            SsTableIterInner::Double { back, .. } => back,
        }
    }

    fn is_same(&self) -> bool {
        matches!(self, SsTableIterInner::Same(_))
    }

    fn reset_front(&mut self, iter: BlockIter) {
        debug_assert!(!self.is_same());
        let SsTableIterInner::Double { front, back: _ } = self else {
            unreachable!()
        };
        *front = iter;
    }

    fn reset_back(&mut self, iter: BlockIter) {
        debug_assert!(!self.is_same());
        let SsTableIterInner::Double { front: _, back } = self else {
            unreachable!()
        };
        *back = iter;
    }

    fn convert_front_as_same(&mut self) {
        debug_assert!(!self.is_same());
        let SsTableIterInner::Double { front, back: _ } = self else {
            unreachable!()
        };
        *self = SsTableIterInner::Same(front.clone());
    }

    fn convert_back_as_same(&mut self) {
        debug_assert!(!self.is_same());
        let SsTableIterInner::Double { front: _, back } = self else {
            unreachable!()
        };
        *self = SsTableIterInner::Same(back.clone());
    }
}
#[cfg(test)]
mod test {

    use crate::block::BlockBuilder;

    use super::*;
    use std::sync::Arc;
    #[test]
    fn block_double_end_iter() {
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new(Arc::new(block));
        println!("{:?}", iter);
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn block_range_iter() {
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_scan(
            Arc::new(block),
            Bound::Included(b"key0"),
            Bound::Included(b"key4"),
        );
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());

        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_scan(
            Arc::new(block),
            Bound::Included(b"key1"),
            Bound::Included(b"key3"),
        );
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());

        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key0", b"value0");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_scan(
            Arc::new(block),
            Bound::Included(b"key1"),
            Bound::Included(b"key3"),
        );
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k2, v2) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key2"));
        assert_eq!(v1, Bytes::from_static(b"value2"));
        assert_eq!(k2, Bytes::from_static(b"key3"));
        assert_eq!(v2, Bytes::from_static(b"value3"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn block_scan() {
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter =
            BlockIter::new_scan(Arc::new(block), Bound::Excluded(b"key1"), Bound::Unbounded);
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k2, v2) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key2"));
        assert_eq!(v1, Bytes::from_static(b"value2"));
        assert_eq!(k2, Bytes::from_static(b"key3"));
        assert_eq!(v2, Bytes::from_static(b"value3"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn block_scan2() {
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter =
            BlockIter::new_scan(Arc::new(block), Bound::Unbounded, Bound::Excluded(b"key3"));
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k2, v2) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn block_double_end_iter_with_delete() {
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key4", b"");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new(Arc::new(block));
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        let (k4, v4) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert_eq!(k4, Bytes::from_static(b"key4"));
        assert_eq!(v4, Bytes::new());
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn sstable_iter() {
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key3"), Bytes::from_static(b"value3"));
        let table = builder.build();
        let mut iter = table.iter();
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn sstable_iter_with_delete() {
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key4"), Bytes::new());
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key5"), Bytes::new());
        builder.add(Bytes::from_static(b"key3"), Bytes::from_static(b"value3"));
        let table = builder.build();
        let mut iter = table.iter();
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k4, v4) = Iterator::next(&mut iter).unwrap();
        let (k5, v5) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key1"));
        assert_eq!(v1, Bytes::from_static(b"value1"));
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert_eq!(k4, Bytes::from_static(b"key4"));
        assert_eq!(v4, Bytes::new());
        assert_eq!(k5, Bytes::from_static(b"key5"));
        assert_eq!(v5, Bytes::new());
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn sstable_scan() {
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key4"), Bytes::new());
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key5"), Bytes::new());
        builder.add(Bytes::from_static(b"key3"), Bytes::from_static(b"value3"));
        let table = builder.build();
        assert!(table.contains_key(b"key1"));
        let mut iter = SsTableIter::new_scan(&table, Bound::Excluded(b"key1"), Bound::Unbounded);
        let (k3, v3) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k4, v4) = Iterator::next(&mut iter).unwrap();
        let (k5, v5) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        let (k2, v2) = Iterator::next(&mut iter).unwrap();
        assert_eq!(k3, Bytes::from_static(b"key3"));
        assert_eq!(v3, Bytes::from_static(b"value3"));
        assert_eq!(k4, Bytes::from_static(b"key4"));
        assert_eq!(v4, Bytes::new());
        assert_eq!(k5, Bytes::from_static(b"key5"));
        assert_eq!(v5, Bytes::new());
        assert_eq!(k2, Bytes::from_static(b"key2"));
        assert_eq!(v2, Bytes::from_static(b"value2"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }

    #[test]
    fn sstable_import_checksum() {
        // Create an SSTable in memory
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key3"), Bytes::from_static(b"value3"));
        let original_table = builder.build();
        let mut buffer = original_table.export_all().to_vec();
        buffer[11] = 123;
        assert!(SsTable::import_all(buffer.into()).is_err());
    }
}
