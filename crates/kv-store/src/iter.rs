use std::{
    fmt::Debug,
    ops::{Bound, Range},
    sync::Arc,
};

use bytes::{Buf, Bytes};

use super::sstable::{Block, SIZE_OF_U16, SIZE_OF_U8};

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
            Block::Large(block) => {
                self.next_key = block.key.to_vec();
                self.next_value_range = 0..block.value_bytes.len();
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
            Block::Large(block) => {
                self.prev_key = block.key.to_vec();
                self.prev_value_range = 0..block.value_bytes.len();
            }
        }
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
