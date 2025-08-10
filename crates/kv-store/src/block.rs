use std::{
    fmt::Debug,
    io::Write,
    ops::{Bound, Range},
    sync::Arc,
};

use bytes::{Buf, Bytes};
use loro_common::LoroResult;
use once_cell::sync::OnceCell;

use crate::{
    compress::{compress, decompress, CompressionType},
    iter::KvIterator,
    sstable::{get_common_prefix_len_and_strip, SIZE_OF_U32, XXH_SEED},
};

use super::sstable::{SIZE_OF_U16, SIZE_OF_U8};

#[derive(Debug, Clone)]
pub struct LargeValueBlock {
    // without checksum
    pub value_bytes: Bytes,
    pub encoded_bytes: OnceCell<(Bytes, CompressionType)>,
    pub key: Bytes,
}

impl LargeValueBlock {
    /// ┌──────────────────────────┐
    /// │Large Block               │
    /// │┌ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ │
    /// │  value   Block Checksum ││
    /// ││ bytes │      u32        │
    /// │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘│
    /// └──────────────────────────┘
    fn encode(&self, w: &mut Vec<u8>, mut compression_type: CompressionType) -> CompressionType {
        if let Some((bytes, encoded_compression_type)) = self.encoded_bytes.get() {
            if encoded_compression_type == &compression_type {
                w.extend_from_slice(bytes);
                return compression_type;
            }
        }

        let origin_len = w.len();
        compress(w, &self.value_bytes, compression_type);
        if !compression_type.is_none() && w.len() - origin_len > self.value_bytes.len() {
            w.truncate(origin_len);
            compress(w, &self.value_bytes, CompressionType::None);
            ensure_cov::notify_cov("kv_store::block::LargeValueBlock::encode::compress_fallback");
            compression_type = CompressionType::None;
        }
        let checksum = xxhash_rust::xxh32::xxh32(&w[origin_len..], XXH_SEED);
        w.write_all(&checksum.to_le_bytes()).unwrap();
        compression_type
    }

    fn decode(bytes: Bytes, key: Bytes, compression_type: CompressionType) -> LoroResult<Self> {
        let mut value_bytes = vec![];
        decompress(
            &mut value_bytes,
            bytes.slice(..bytes.len() - SIZE_OF_U32),
            compression_type,
        )?;
        Ok(LargeValueBlock {
            value_bytes: Bytes::from(value_bytes),
            encoded_bytes: OnceCell::with_value((bytes, compression_type)),
            key,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NormalBlock {
    pub data: Bytes,
    pub encoded_data: OnceCell<(Bytes, CompressionType)>,
    pub first_key: Bytes,
    pub offsets: Vec<u16>,
}

impl NormalBlock {
    /// ┌────────────────────────────────────────────────────────────────────────────────────────┐
    /// │Block                                                                                   │
    /// │┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ┬ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ │
    /// │ Key Value Chunk  ...  │Key Value Chunk  offset │ ...  │ offset  kv len │Block Checksum││
    /// ││     bytes     │      │     bytes     │  u16   │      │  u16  │  u16   │     u32       │
    /// │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ┘│
    /// └────────────────────────────────────────────────────────────────────────────────────────┘
    ///
    /// The block body may be compressed then we calculate its checksum (the checksum is not compressed).
    fn encode(&self, w: &mut Vec<u8>, mut compression_type: CompressionType) -> CompressionType {
        if let Some((encoded_data, encoded_compression_type)) = self.encoded_data.get() {
            if encoded_compression_type == &compression_type {
                w.extend_from_slice(encoded_data);
                return compression_type;
            }
        }

        let origin_len = w.len();
        let mut buf = self.data.to_vec();
        for offset in &self.offsets {
            buf.extend_from_slice(&offset.to_le_bytes());
        }
        buf.extend_from_slice(&(self.offsets.len() as u16).to_le_bytes());
        compress(w, &buf, compression_type);
        if !compression_type.is_none() && w.len() - origin_len > buf.len() {
            w.truncate(origin_len);
            compress(w, &buf, CompressionType::None);
            ensure_cov::notify_cov("kv_store::block::NormalBlock::encode::compress_fallback");
            compression_type = CompressionType::None;
        }
        let checksum = xxhash_rust::xxh32::xxh32(&w[origin_len..], XXH_SEED);
        w.extend_from_slice(&checksum.to_le_bytes());
        compression_type
    }

    fn decode(
        raw_block_and_check: Bytes,
        first_key: Bytes,
        compression_type: CompressionType,
    ) -> LoroResult<NormalBlock> {
        let buf = raw_block_and_check.slice(..raw_block_and_check.len() - SIZE_OF_U32);
        let mut data = vec![];
        decompress(&mut data, buf, compression_type)?;
        let offsets_len = (&data[data.len() - SIZE_OF_U16..]).get_u16_le() as usize;
        let data_end = data.len() - SIZE_OF_U16 * (offsets_len + 1);
        let offsets = &data[data_end..data.len() - SIZE_OF_U16];
        let offsets = offsets
            .chunks(SIZE_OF_U16)
            .map(|mut chunk| chunk.get_u16_le())
            .collect();
        Ok(NormalBlock {
            data: Bytes::copy_from_slice(&data[..data_end]),
            encoded_data: OnceCell::with_value((raw_block_and_check, compression_type)),
            offsets,
            first_key,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Block {
    Normal(NormalBlock),
    Large(LargeValueBlock),
}

impl Block {
    pub fn is_large(&self) -> bool {
        matches!(self, Block::Large(_))
    }

    pub fn data(&self) -> Bytes {
        match self {
            Block::Normal(block) => block.data.clone(),
            Block::Large(block) => block.value_bytes.clone(),
        }
    }

    pub fn first_key(&self) -> Bytes {
        match self {
            Block::Normal(block) => block.first_key.clone(),
            Block::Large(block) => block.key.clone(),
        }
    }

    pub fn last_key(&self) -> Bytes {
        match self {
            Block::Normal(block) => {
                if block.offsets.len() == 1 {
                    return block.first_key.clone();
                }

                let offset = *block.offsets.last().unwrap() as usize;
                let mut bytes = &block.data[offset..];
                let common_prefix_len = bytes.get_u8() as usize;
                let key_suffix_len = bytes.get_u16_le() as usize;
                let mut last_key = Vec::with_capacity(common_prefix_len + key_suffix_len);
                last_key.extend_from_slice(&block.first_key[..common_prefix_len]);
                last_key.extend_from_slice(&bytes[..key_suffix_len]);
                last_key.into()
            }
            Block::Large(block) => block.key.clone(),
        }
    }

    pub fn encode(&self, w: &mut Vec<u8>, compression_type: CompressionType) -> CompressionType {
        match self {
            Block::Normal(block) => block.encode(w, compression_type),
            Block::Large(block) => block.encode(w, compression_type),
        }
    }

    pub fn decode(
        raw_block_and_check: Bytes,
        is_large: bool,
        key: Bytes,
        compression_type: CompressionType,
    ) -> Self {
        // we have checked the checksum, so the block should be valid when decompressing
        if is_large {
            return LargeValueBlock::decode(raw_block_and_check, key, compression_type)
                .map(Block::Large)
                .unwrap();
        }
        NormalBlock::decode(raw_block_and_check, key, compression_type)
            .map(Block::Normal)
            .unwrap()
    }

    pub fn len(&self) -> usize {
        match self {
            Block::Normal(block) => block.offsets.len(),
            Block::Large(_) => 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Block::Normal(block) => block.offsets.is_empty(),
            Block::Large(_) => false,
        }
    }
}

#[derive(Debug)]
pub struct BlockBuilder {
    data: Vec<u8>,
    offsets: Vec<u16>,
    block_size: usize,
    // for key compression
    first_key: Bytes,
    is_large: bool,
}

impl BlockBuilder {
    pub fn new(block_size: usize) -> Self {
        Self {
            data: Vec::new(),
            offsets: Vec::new(),
            block_size,
            first_key: Bytes::new(),
            is_large: false,
        }
    }

    pub fn estimated_size(&self) -> usize {
        if self.is_large {
            self.data.len()
        } else {
            // key-value pairs number
            SIZE_OF_U16 +
            // offsets
            self.offsets.len() * SIZE_OF_U16 +
            // key-value pairs data
            self.data.len() +
            // checksum
            SIZE_OF_U32
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.is_large && self.offsets.is_empty()
    }

    /// Add a key-value pair to the block.
    /// Returns true if the key-value pair is added successfully, false the block is full.
    ///
    /// ┌─────────────────────────────────────────────────────┐
    /// │  Key Value Chunk                                    │
    /// │┌ ─ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─┬ ─ ─ ─ ┐│
    /// │ common prefix len key suffix len│key suffix│ value ││
    /// ││       u8        │     u16      │  bytes   │ bytes ││
    /// │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘ ─ ─ ─ ┘│
    /// └─────────────────────────────────────────────────────┘
    ///
    pub fn add(&mut self, key: &[u8], value: &[u8]) -> bool {
        debug_assert!(!key.is_empty(), "key cannot be empty");
        if self.first_key.is_empty() {
            if value.len() > self.block_size {
                self.data.extend_from_slice(value);
                self.is_large = true;
                self.first_key = Bytes::copy_from_slice(key);
                return true;
            }

            self.first_key = Bytes::copy_from_slice(key);
            self.offsets.push(self.data.len() as u16);
            self.data.extend_from_slice(value);
            return true;
        }

        // whether the block is full
        if self.estimated_size() + key.len() + value.len() + SIZE_OF_U8 + SIZE_OF_U16
            > self.block_size
        {
            return false;
        }

        self.offsets.push(self.data.len() as u16);
        let (common, suffix) = get_common_prefix_len_and_strip(key, &self.first_key);
        let key_len = suffix.len();
        self.data.push(common);
        self.data.extend_from_slice(&(key_len as u16).to_le_bytes());
        self.data.extend_from_slice(suffix);
        self.data.extend_from_slice(value);
        true
    }

    pub fn build(self) -> Block {
        if self.is_large {
            return Block::Large(LargeValueBlock {
                value_bytes: Bytes::from(self.data),
                key: self.first_key,
                encoded_bytes: OnceCell::new(),
            });
        }
        debug_assert!(!self.offsets.is_empty(), "block is empty");
        Block::Normal(NormalBlock {
            data: Bytes::from(self.data),
            offsets: self.offsets,
            first_key: self.first_key,
            encoded_data: OnceCell::new(),
        })
    }
}

/// Block iterator
///
/// If the key is empty, it means the iterator is invalid.
#[derive(Clone)]
pub struct BlockIter {
    block: Arc<Block>,
    next_key: Bytes,
    next_value_range: Range<usize>,
    prev_key: Bytes,
    prev_value_range: Range<usize>,
    next_idx: usize,
    prev_idx: isize,
    first_key: Bytes,
}

impl Debug for BlockIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockIter")
            .field("is_large", &self.block.is_large())
            .field("next_key", &self.next_key)
            .field("next_value_range", &self.next_value_range)
            .field("prev_key", &self.prev_key)
            .field("prev_value_range", &self.prev_value_range)
            .field("next_idx", &self.next_idx)
            .field("prev_idx", &self.prev_idx)
            .field("first_key", &Bytes::copy_from_slice(&self.first_key))
            .finish()
    }
}

impl BlockIter {
    pub fn new(block: Arc<Block>) -> Self {
        let prev_idx = block.len() as isize - 1;
        let mut iter = Self {
            first_key: block.first_key(),
            block,
            next_key: Bytes::new(),
            next_value_range: 0..0,
            prev_key: Bytes::new(),
            prev_value_range: 0..0,
            next_idx: 0,
            prev_idx,
        };
        iter.seek_to_idx(0);
        iter.back_to_idx(prev_idx);
        iter
    }

    pub fn new_seek_to_key(block: Arc<Block>, key: &[u8]) -> Self {
        let prev_idx = block.len() as isize - 1;
        let mut iter = Self {
            first_key: block.first_key(),
            block,
            next_key: Bytes::new(),
            next_value_range: 0..0,
            prev_key: Bytes::new(),
            prev_value_range: 0..0,
            next_idx: 0,
            prev_idx,
        };
        iter.seek_to_key(key);
        iter.back_to_idx(prev_idx);
        iter
    }

    pub fn new_back_to_key(block: Arc<Block>, key: &[u8]) -> Self {
        let prev_idx = block.len() as isize - 1;
        let mut iter = Self {
            first_key: block.first_key(),
            block,
            next_key: Bytes::new(),
            next_value_range: 0..0,
            prev_key: Bytes::new(),
            prev_value_range: 0..0,
            next_idx: 0,
            prev_idx,
        };
        iter.seek_to_idx(0);
        iter.back_to_key(key);
        iter
    }

    pub fn new_scan(block: Arc<Block>, start: Bound<&[u8]>, end: Bound<&[u8]>) -> Self {
        let mut iter = match start {
            Bound::Included(key) => Self::new_seek_to_key(block, key),
            Bound::Excluded(key) => {
                let mut iter = Self::new_seek_to_key(block, key);
                while iter.has_next() && iter.peek_next_curr_key().unwrap() == key {
                    iter.next();
                }
                iter
            }
            Bound::Unbounded => Self::new(block),
        };
        match end {
            Bound::Included(key) => {
                iter.back_to_key(key);
            }
            Bound::Excluded(key) => {
                iter.back_to_key(key);
                while iter.has_next_back() && iter.peek_back_curr_key().unwrap() == key {
                    iter.next_back();
                }
            }
            Bound::Unbounded => {}
        }
        iter
    }

    pub fn peek_next_curr_key(&self) -> Option<Bytes> {
        if self.has_next() {
            Some(Bytes::copy_from_slice(&self.next_key))
        } else {
            None
        }
    }

    pub fn peek_next_curr_value(&self) -> Option<Bytes> {
        if self.has_next() {
            Some(self.block.data().slice(self.next_value_range.clone()))
        } else {
            None
        }
    }

    pub fn has_next(&self) -> bool {
        !self.next_key.is_empty() && self.next_idx as isize <= self.prev_idx
    }

    pub fn peek_back_curr_key(&self) -> Option<Bytes> {
        if self.has_next_back() {
            Some(Bytes::copy_from_slice(&self.prev_key))
        } else {
            None
        }
    }

    pub fn peek_back_curr_value(&self) -> Option<Bytes> {
        if self.has_next_back() {
            Some(self.block.data().slice(self.prev_value_range.clone()))
        } else {
            None
        }
    }

    pub fn has_next_back(&self) -> bool {
        !self.prev_key.is_empty() && self.next_idx as isize <= self.prev_idx
    }

    pub fn next(&mut self) {
        self.next_idx += 1;
        if self.next_idx as isize > self.prev_idx {
            self.next_key.clear();
            self.next_value_range = 0..0;
            return;
        }
        self.seek_to_idx(self.next_idx);
    }

    pub fn next_back(&mut self) {
        self.prev_idx -= 1;
        if self.prev_idx < 0 || self.prev_idx < (self.next_idx as isize) {
            self.prev_key.clear();
            self.prev_value_range = 0..0;
            return;
        }
        self.back_to_idx(self.prev_idx);
    }

    pub fn seek_to_key(&mut self, key: &[u8]) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                let mut left = 0;
                let mut right = block.offsets.len();
                while left < right {
                    let mid = left + (right - left) / 2;
                    self.seek_to_idx(mid);
                    debug_assert!(self.has_next());
                    if self.next_key == key {
                        return;
                    }
                    if self.next_key < key {
                        left = mid + 1;
                    } else {
                        right = mid;
                    }
                }
                self.seek_to_idx(left);
            }
            Block::Large(block) => {
                if key > block.key {
                    self.seek_to_idx(1);
                } else {
                    self.seek_to_idx(0);
                }
            }
        }
    }

    /// MUST be called after seek_to_key()
    pub fn back_to_key(&mut self, key: &[u8]) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                let mut left = self.next_idx;
                let mut right = block.offsets.len();
                while left < right {
                    let mid = left + (right - left) / 2;
                    self.back_to_idx(mid as isize);
                    // prev idx <= next idx
                    if !self.has_next_back() {
                        return;
                    }
                    debug_assert!(self.has_next_back());
                    if self.prev_key > key {
                        right = mid;
                    } else {
                        left = mid + 1;
                    }
                }
                self.back_to_idx(left as isize - 1);
            }
            Block::Large(block) => {
                if key < block.key {
                    self.back_to_idx(-1);
                } else {
                    self.back_to_idx(0);
                }
            }
        }
    }

    fn seek_to_idx(&mut self, idx: usize) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                if idx >= block.offsets.len() {
                    self.next_key.clear();
                    self.next_value_range = 0..0;
                    self.next_idx = idx;
                    return;
                }
                let offset = block.offsets[idx] as usize;
                self.seek_to_offset(
                    offset,
                    *block
                        .offsets
                        .get(idx + 1)
                        .unwrap_or(&(block.data.len() as u16)) as usize,
                    idx == 0,
                );
                self.next_idx = idx;
            }
            Block::Large(block) => {
                if idx > 0 {
                    self.next_key.clear();
                    self.next_value_range = 0..0;
                    self.next_idx = idx;
                    return;
                }
                self.next_key = block.key.clone();
                self.next_value_range = 0..block.value_bytes.len();
                self.next_idx = idx;
            }
        }
    }

    fn back_to_idx(&mut self, idx: isize) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                if idx < 0 {
                    self.prev_key.clear();
                    self.prev_value_range = 0..0;
                    self.prev_idx = idx;
                    return;
                }
                let offset = block.offsets[idx as usize] as usize;
                self.back_to_offset(
                    offset,
                    *block
                        .offsets
                        .get(idx as usize + 1)
                        .unwrap_or(&(block.data.len() as u16)) as usize,
                    idx == 0,
                );
                self.prev_idx = idx;
            }
            Block::Large(block) => {
                if idx < 0 {
                    self.prev_key.clear();
                    self.prev_value_range = 0..0;
                    self.prev_idx = idx;
                    return;
                }
                self.prev_key = block.key.clone();
                self.prev_value_range = 0..block.value_bytes.len();
                self.prev_idx = idx;
            }
        }
    }

    fn seek_to_offset(&mut self, offset: usize, offset_end: usize, is_first: bool) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                if is_first {
                    self.next_key = self.first_key.clone();
                    self.next_value_range = offset..offset_end;
                    return;
                }
                let mut rest = &block.data[offset..];
                let common_prefix_len = rest.get_u8() as usize;
                let key_suffix_len = rest.get_u16_le() as usize;
                let mut next_key = Vec::with_capacity(common_prefix_len + key_suffix_len);
                next_key.extend_from_slice(&self.first_key[..common_prefix_len]);
                next_key.extend_from_slice(&rest[..key_suffix_len]);
                self.next_key = next_key.into();
                let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len;
                self.next_value_range = value_start..offset_end;
            }
            Block::Large(_) => {
                unreachable!()
            }
        }
    }

    fn back_to_offset(&mut self, offset: usize, offset_end: usize, is_first: bool) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                if is_first {
                    self.prev_key = self.first_key.clone();
                    self.prev_value_range = offset..offset_end;
                    return;
                }
                let mut rest = &block.data[offset..];
                let common_prefix_len = rest.get_u8() as usize;
                let key_suffix_len = rest.get_u16_le() as usize;
                let mut prev_key = Vec::with_capacity(common_prefix_len + key_suffix_len);
                prev_key.extend_from_slice(&self.first_key[..common_prefix_len]);
                prev_key.extend_from_slice(&rest[..key_suffix_len]);
                self.prev_key = prev_key.into();
                let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len;
                self.prev_value_range = value_start..offset_end;
            }
            Block::Large(_) => {
                unreachable!()
            }
        }
    }

    pub fn peek_block(&self) -> &Arc<Block> {
        &self.block
    }

    pub fn finish(&mut self) {
        self.next_key.clear();
        self.next_value_range = 0..0;
        self.prev_key.clear();
        self.prev_value_range = 0..0;
    }
}

impl KvIterator for BlockIter {
    fn peek_next_key(&self) -> Option<Bytes> {
        self.peek_next_curr_key()
    }

    fn peek_next_value(&self) -> Option<Bytes> {
        self.peek_next_curr_value()
    }

    fn next_(&mut self) {
        self.next();
    }

    fn has_next(&self) -> bool {
        self.has_next()
    }

    fn peek_next_back_key(&self) -> Option<Bytes> {
        self.peek_back_curr_key()
    }

    fn peek_next_back_value(&self) -> Option<Bytes> {
        self.peek_back_curr_value()
    }

    fn next_back_(&mut self) {
        self.next_back();
    }

    fn has_next_back(&self) -> bool {
        self.has_next_back()
    }
}

impl Iterator for BlockIter {
    type Item = (Bytes, Bytes);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next() {
            return None;
        }
        let key = self.peek_next_curr_key().unwrap();
        let value = self.peek_next_curr_value().unwrap();
        self.next();
        Some((key, value))
    }
}

impl DoubleEndedIterator for BlockIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        if !self.has_next_back() {
            return None;
        }
        let key = self.peek_back_curr_key().unwrap();
        let value = self.peek_back_curr_value().unwrap();
        self.next_back();
        Some((key, value))
    }
}
