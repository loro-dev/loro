use std::{
    fmt::Debug,
    ops::{Bound, Range},
    sync::Arc,
};

use bytes::{Buf, BufMut, Bytes};

use crate::{iter::KvIterator, sstable::{get_common_prefix_len_and_strip, SIZE_OF_U32, XXH_SEED}};

use super::sstable::{ SIZE_OF_U16, SIZE_OF_U8};


#[derive(Debug)]
pub struct LargeValueBlock{
    // without checksum
    pub value_bytes: Bytes,
    pub key: Bytes,
}

impl LargeValueBlock{
    /// ┌──────────────────────────┐
    /// │Large Block               │
    /// │┌ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ │
    /// │  value   Block Checksum ││
    /// ││ bytes │      u32        │
    /// │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘│
    /// └──────────────────────────┘
    fn encode(&self)->Bytes{
        let mut buf = Vec::with_capacity(self.value_bytes.len() + SIZE_OF_U32);
        buf.put_slice(&self.value_bytes);
        let checksum = xxhash_rust::xxh32::xxh32(&buf, XXH_SEED);
        buf.put_u32(checksum);
        buf.into()
    }

    fn decode(bytes:Bytes, key: Bytes)->Self{
        LargeValueBlock{
            value_bytes: bytes.slice(..bytes.len() - SIZE_OF_U32),
            key,
        }
    }
}

#[derive(Debug)]
pub struct NormalBlock {
    pub data: Bytes,
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
    /// check sum will be calculated by crc32 later
    fn encode(&self) -> Bytes {
        let mut buf = self.data.to_vec();
        for offset in &self.offsets {
            buf.put_u16(*offset);
        }
        buf.put_u16(self.offsets.len() as u16);

        let mut compressed_data = lz4_flex::compress_prepend_size(&buf);

        let checksum = xxhash_rust::xxh32::xxh32(&compressed_data, XXH_SEED);
        compressed_data.put_u32(checksum);
        compressed_data.into()
    }

    fn decode(raw_block_and_check: Bytes, first_key: Bytes)-> NormalBlock{
        let data = raw_block_and_check.slice(..raw_block_and_check.len() - SIZE_OF_U32);
        let data = lz4_flex::decompress_size_prepended(&data).unwrap();
        let offsets_len = (&data[data.len()-SIZE_OF_U16..]).get_u16() as usize;
        let data_end = data.len() - SIZE_OF_U16 * (offsets_len + 1);
        let offsets = &data[data_end..data.len()-SIZE_OF_U16];
        let offsets = offsets.chunks(SIZE_OF_U16).map(|mut chunk| chunk.get_u16()).collect();
        NormalBlock{
            data: Bytes::copy_from_slice(&data[..data_end]),
            offsets,
            first_key,
        }
    }
}

#[derive(Debug)]
pub enum Block{
    Normal(NormalBlock),
    Large(LargeValueBlock),
}

impl Block{
    pub fn is_large(&self)->bool{
        matches!(self, Block::Large(_))
    }

    pub fn data(&self)->Bytes{
        match self{
            Block::Normal(block)=>block.data.clone(),
            Block::Large(block)=>block.value_bytes.clone(),
        }
    }

    pub fn first_key(&self)->Bytes{
        match self{
            Block::Normal(block)=>block.first_key.clone(),
            Block::Large(block)=>block.key.clone(),
        }
    }

    pub fn encode(&self)->Bytes{
        match self{
            Block::Normal(block)=>block.encode(),
            Block::Large(block)=>block.encode(),
        }
    }

    pub fn decode(raw_block_and_check: Bytes, is_large: bool, key: Bytes)->Self{
        if is_large{
            return Block::Large(LargeValueBlock::decode(raw_block_and_check, key));
        }
        Block::Normal(NormalBlock::decode(raw_block_and_check, key))
    }

    pub fn len(&self)->usize{
        match self{
            Block::Normal(block)=>block.offsets.len(),
            Block::Large(_)=>1,
        }
    }

    pub fn is_empty(&self)->bool{
        match self{
            Block::Normal(block)=>block.offsets.is_empty(),
            Block::Large(_)=>false,
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
            is_large:false
        }
    }

    fn estimated_size(&self) -> usize {
        if self.is_large{
            self.data.len()
        }else{
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

    pub fn is_empty(&self)->bool{
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
        if  self.first_key.is_empty() && value.len() > self.block_size {
            self.data.put(value);
            self.is_large = true;
            self.first_key = Bytes::copy_from_slice(key);
            return true;
        }

        // whether the block is full
        if self.estimated_size() + key.len() + value.len() + SIZE_OF_U8 + SIZE_OF_U16 > self.block_size && !self.first_key.is_empty() {
            return false;
        }

        if self.first_key.is_empty() {
            self.first_key = Bytes::copy_from_slice(key);
        }
        self.offsets.push(self.data.len() as u16);
        let (common, suffix) = get_common_prefix_len_and_strip(key, &self.first_key);
        let key_len = suffix.len() ;
        self.data.put_u8(common);
        self.data.put_u16(key_len as u16);
        self.data.put(suffix);
        self.data.put(value);
        true
    }

    pub fn build(self)->Block{
        if self.is_large{
            return Block::Large(LargeValueBlock{
                value_bytes: Bytes::from(self.data),
                key: self.first_key,
            });
        }
        debug_assert!(!self.offsets.is_empty(), "block is empty");
        Block::Normal(NormalBlock{
            data: Bytes::from(self.data),
            offsets: self.offsets,
            first_key: self.first_key,
        })
    }
}


#[derive(Clone)]
pub struct BlockIter {
    block: Arc<Block>,
    next_key: Vec<u8>,
    next_value_range: Range<usize>,
    prev_key: Vec<u8>,
    prev_value_range: Range<usize>,
    next_idx: usize,
    prev_idx: isize,
    first_key: Bytes,
}

impl Debug for BlockIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockIter")
            .field("is_large", &self.block.is_large())
            .field("next_key", &Bytes::copy_from_slice(&self.next_key))
            .field("next_value_range", &self.next_value_range)
            .field("prev_key", &Bytes::copy_from_slice(&self.prev_key))
            .field("prev_value_range", &self.prev_value_range)
            .field("next_idx", &self.next_idx)
            .field("prev_idx", &self.prev_idx)
            .field("first_key", &Bytes::copy_from_slice(&self.first_key))
            .finish()
    }
}

impl BlockIter {
    pub fn new_seek_to_first(block: Arc<Block>) -> Self {
        let prev_idx = block.len() as isize - 1;
        let mut iter = Self {
            first_key: block.first_key(),
            block,
            next_key: Vec::new(),
            next_value_range: 0..0,
            prev_key: Vec::new(),
            prev_value_range: 0..0,
            next_idx: 0,
            prev_idx,
        };
        iter.seek_to_idx(0);
        iter.prev_to_idx(prev_idx);
        iter
    }

    pub fn new_seek_to_key(block: Arc<Block>, key: &[u8]) -> Self {
        let prev_idx = block.len() as isize - 1;
        let mut iter = Self {
            first_key: block.first_key(),
            block,
            next_key: Vec::new(),
            next_value_range: 0..0,
            prev_key: Vec::new(),
            prev_value_range: 0..0,
            next_idx: 0,
            prev_idx,
        };
        iter.seek_to_key(key);
        iter.prev_to_idx(prev_idx);
        iter
    }

    pub fn new_prev_to_key(block: Arc<Block>, key: &[u8]) -> Self {
        let prev_idx = block.len() as isize - 1;
        let mut iter = Self {
            first_key: block.first_key(),
            block,
            next_key: Vec::new(),
            next_value_range: 0..0,
            prev_key: Vec::new(),
            prev_value_range: 0..0,
            next_idx: 0,
            prev_idx,
        };
        iter.seek_to_idx(0);
        iter.prev_to_key(key);
        iter
    }

    pub fn new_scan(block: Arc<Block>, start: Bound<&[u8]>, end: Bound<&[u8]>) -> Self {
        let mut iter = match start {
            Bound::Included(key) => Self::new_seek_to_key(block, key),
            Bound::Excluded(key) => {
                let mut iter = Self::new_seek_to_key(block, key);
                while iter.next_is_valid() && iter.next_curr_key() == key {
                    iter.next();
                }
                iter
            }
            Bound::Unbounded => Self::new_seek_to_first(block),
        };
        match end {
            Bound::Included(key) => {
                iter.prev_to_key(key);
            }
            Bound::Excluded(key) => {
                iter.prev_to_key(key);
                while iter.prev_is_valid() && iter.prev_curr_key() == key {
                    iter.prev();
                }
            }
            Bound::Unbounded => {}
        }
        iter
    }

    pub fn next_curr_key(&self) -> Bytes {
        debug_assert!(self.next_is_valid());
        Bytes::copy_from_slice(&self.next_key)
    }

    pub fn next_curr_value(&self) -> Bytes {
        debug_assert!(self.next_is_valid());
        self.block.data().slice(self.next_value_range.clone())
    }

    pub fn next_is_valid(&self) -> bool {
        !self.next_key.is_empty() && self.next_idx as isize <= self.prev_idx
    }

    pub fn prev_curr_key(&self) -> Bytes {
        debug_assert!(self.prev_is_valid());
        Bytes::copy_from_slice(&self.prev_key)
    }

    pub fn prev_curr_value(&self) -> Bytes {
        debug_assert!(self.prev_is_valid());
        self.block.data().slice(self.prev_value_range.clone())
    }

    pub fn prev_is_valid(&self) -> bool {
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

    pub fn prev(&mut self) {
        self.prev_idx -= 1;
        if self.prev_idx < 0 || self.prev_idx < (self.next_idx as isize) {
            self.prev_key.clear();
            self.prev_value_range = 0..0;
            return;
        }
        self.prev_to_idx(self.prev_idx);
    }

    pub fn seek_to_key(&mut self, key: &[u8]) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                let mut left = 0;
                let mut right = block.offsets.len();
                while left < right {
                    let mid = left + (right - left) / 2;
                    self.seek_to_idx(mid);
                    debug_assert!(self.next_is_valid());
                    if self.next_key.as_slice() == key {
                        return;
                    }
                    if self.next_key.as_slice() < key {
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
    pub fn prev_to_key(&mut self, key: &[u8]) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                let mut left = self.next_idx;
                let mut right = block.offsets.len();
                while left < right {
                    let mid = left + (right - left) / 2;
                    self.prev_to_idx(mid as isize);
                    // prev idx <= next idx
                    if !self.prev_is_valid() {
                        return;
                    }
                    debug_assert!(self.prev_is_valid());
                    if self.prev_key.as_slice() > key {
                        right = mid;
                    } else {
                        left = mid + 1;
                    }
                }
                self.prev_to_idx(left as isize - 1);
            }
            Block::Large(block) => {
                if key < block.key {
                    self.prev_to_idx(-1);
                } else {
                    self.prev_to_idx(0);
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
                self.next_key = block.key.to_vec();
                self.next_value_range = 0..block.value_bytes.len();
                self.next_idx = idx;
            }
        }
    }

    fn prev_to_idx(&mut self, idx: isize) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                if idx < 0 {
                    self.prev_key.clear();
                    self.prev_value_range = 0..0;
                    self.prev_idx = idx;
                    return;
                }
                let offset = block.offsets[idx as usize] as usize;
                self.prev_to_offset(
                    offset,
                    *block
                        .offsets
                        .get(idx as usize + 1)
                        .unwrap_or(&(block.data.len() as u16)) as usize,
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
                self.prev_key = block.key.to_vec();
                self.prev_value_range = 0..block.value_bytes.len();
                self.prev_idx = idx;
            }
        }
    }

    fn seek_to_offset(&mut self, offset: usize, offset_end: usize) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                let mut rest = &block.data[offset..];
                let common_prefix_len = rest.get_u8() as usize;
                let key_suffix_len = rest.get_u16() as usize;
                self.next_key.clear();
                self.next_key
                    .extend_from_slice(&self.first_key[..common_prefix_len]);
                self.next_key.extend_from_slice(&rest[..key_suffix_len]);
                rest.advance(key_suffix_len);
                let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len;
                self.next_value_range = value_start..offset_end;
                rest.advance(offset_end - value_start);
            }
            Block::Large(_) => {
                unreachable!()
            }
        }
    }

    fn prev_to_offset(&mut self, offset: usize, offset_end: usize) {
        match self.block.as_ref() {
            Block::Normal(block) => {
                let mut rest = &block.data[offset..];
                let common_prefix_len = rest.get_u8() as usize;
                let key_suffix_len = rest.get_u16() as usize;
                self.prev_key.clear();
                self.prev_key
                    .extend_from_slice(&self.first_key[..common_prefix_len]);
                self.prev_key.extend_from_slice(&rest[..key_suffix_len]);
                rest.advance(key_suffix_len);
                let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len;
                self.prev_value_range = value_start..offset_end;
                rest.advance(offset_end - value_start);
            }
            Block::Large(_) => {
                unreachable!()
            }
        }
    }
}

impl KvIterator for BlockIter{
    fn next_key(&self) -> Bytes {
        self.next_curr_key()
    }

    fn next_value(&self) -> Bytes {
        self.next_curr_value()
    }

    fn find_next(&mut self) {
        self.next();    
    }

    fn is_next_valid(&self) -> bool {
        self.next_is_valid()
    }

    fn prev_key(&self) -> Bytes {
        self.prev_curr_key()
    }

    fn prev_value(&self) -> Bytes {
        self.prev_curr_value()
    }

    fn find_prev(&mut self) {
        self.prev();
    }

    fn is_prev_valid(&self) -> bool {
        self.prev_is_valid()
    }
}

impl Iterator for BlockIter {
    type Item = (Bytes, Bytes);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.next_is_valid() {
            return None;
        }
        let key = self.next_curr_key();
        let value = self.next_curr_value();
        self.next();
        Some((key, value))
    }
}

impl DoubleEndedIterator for BlockIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        if !self.prev_is_valid() {
            return None;
        }
        let key = self.prev_curr_key();
        let value = self.prev_curr_value();
        self.prev();
        Some((key, value))
    }
}
