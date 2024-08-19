use bytes::Bytes;
use std::{
    ops::Bound,
    sync::{Arc, Mutex},
};

pub use mem::MemKvStore;

pub trait KvStore: std::fmt::Debug + Send + Sync {
    fn get(&self, key: &[u8]) -> Option<Bytes>;
    fn set(&mut self, key: &[u8], value: Bytes);
    fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool;
    fn remove(&mut self, key: &[u8]);
    fn contains_key(&self, key: &[u8]) -> bool;
    fn scan(
        &self,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_>;
    fn len(&self) -> usize;
    fn size(&self) -> usize;
    fn export_all(&self) -> Bytes;
    fn import_all(&mut self, bytes: Bytes) -> Result<(), String>;
    fn clone_store(&self) -> Arc<Mutex<dyn KvStore>>;
}
  

fn get_common_prefix_len_and_strip<'a, T: AsRef<[u8]> + ?Sized>(this: &'a T, last: &T) -> (u8, &'a [u8]) {
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

mod default_binary_format {
    //! Default binary format for the key-value store.
    //!
    //! It will compress the prefix of the keys that are common with the previous key.

    use bytes::Bytes;

    use super::get_common_prefix_len_and_strip;

    pub fn export_by_scan(store: &impl super::KvStore) -> bytes::Bytes {
        let mut buf = Vec::new();
        let mut last_key: Option<Bytes> = None;
        for (k, v) in store.scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded) {
            {
                // Write the key
                match last_key.take() {
                    None => {
                        leb128::write::unsigned(&mut buf, k.len() as u64).unwrap();
                        buf.extend_from_slice(&k);
                    }
                    Some(last) => {
                        let (common, suffix) = get_common_prefix_len_and_strip(&k, &last);
                        buf.push(common);
                        leb128::write::unsigned(&mut buf, suffix.len() as u64).unwrap();
                        buf.extend_from_slice(suffix);
                    }
                };

                last_key = Some(k);
            }

            // Write the value
            leb128::write::unsigned(&mut buf, v.len() as u64).unwrap();
            buf.extend_from_slice(&v);
        }

        buf.into()
    }

    

    pub fn import(store: &mut impl super::KvStore, bytes: bytes::Bytes) -> Result<(), String> {
        let mut bytes: &[u8] = &bytes;
        let mut last_key = Vec::new();

        while !bytes.is_empty() {
            // Read the key
            let mut key = Vec::new();
            if last_key.is_empty() {
                let key_len = leb128::read::unsigned(&mut bytes).map_err(|e| e.to_string())?;
                let key_len = key_len as usize;
                key.extend_from_slice(&bytes[..key_len]);
                bytes = &bytes[key_len..];
            } else {
                let common_prefix_len = bytes[0] as usize;
                bytes = &bytes[1..];
                key.extend_from_slice(&last_key[..common_prefix_len]);
                let suffix_len = leb128::read::unsigned(&mut bytes).map_err(|e| e.to_string())?;
                let suffix_len = suffix_len as usize;
                key.extend_from_slice(&bytes[..suffix_len]);
                bytes = &bytes[suffix_len..];
            }

            // Read the value
            let value_len = leb128::read::unsigned(&mut bytes).map_err(|e| e.to_string())?;
            let value_len = value_len as usize;
            let value = Bytes::copy_from_slice(&bytes[..value_len]);
            bytes = &bytes[value_len..];

            // Store the key-value pair
            store.set(&key, value);

            last_key = key;
        }

        Ok(())
    }
}

mod sst_binary_format {
    // Reference: https://github.com/skyzh/mini-lsm
    use std::{ fmt::Debug, ops::{Bound, Range}, sync::Arc};

    use bytes::{Buf, BufMut, Bytes};
    use fxhash::FxHashSet;
    use itertools::Itertools;
    use loro_common::{LoroError, LoroResult};
    use once_cell::sync::OnceCell;

    use crate::kv_store::get_common_prefix_len_and_strip;

    const MAGIC_NUMBER: [u8;4] = *b"LORO";
    const CURRENT_SCHEMA_VERSION: u8 = 0;
    const SIZE_OF_U8: usize = std::mem::size_of::<u8>();
    const SIZE_OF_U16: usize = std::mem::size_of::<u16>();
    const SIZE_OF_U32: usize = std::mem::size_of::<u32>();


    #[derive(Clone)]
    pub struct BlockIter{
        block: Arc<Block>,
        next_key: Vec<u8>,
        next_value_range: Range<usize>,
        prev_key: Vec<u8>,
        prev_value_range: Range<usize>,
        next_idx: usize,
        prev_idx: isize,
        first_key: Bytes,
    }

    impl Debug for BlockIter{
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

    impl BlockIter{
        pub fn new_seek_to_first(block: Arc<Block>)->Self{
            let prev_idx = block.len() as isize - 1;
            let mut iter = Self{
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

        pub fn new_seek_to_key(block: Arc<Block>, key: &[u8])->Self{
            let prev_idx = block.len() as isize - 1;
            let mut iter = Self{
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

        pub fn new_prev_to_key(block: Arc<Block>, key: &[u8])->Self{
            let prev_idx = block.len() as isize - 1;
            let mut iter = Self{
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

        pub fn new_scan(block: Arc<Block>, start: Bound<&[u8]>, end: Bound<&[u8]>)->Self{
            let mut iter = match start{
                Bound::Included(key)=>Self::new_seek_to_key(block, key),
                Bound::Excluded(key)=>{
                    let mut iter = Self::new_seek_to_key(block, key); 
                    while iter.next_is_valid() && iter.next_curr_key() == key{
                        iter.next();
                    }
                    iter
                },
                Bound::Unbounded=>Self::new_seek_to_first(block),
            };
            match end{
                Bound::Included(key)=>{
                    iter.prev_to_key(key);
                }
                Bound::Excluded(key)=>{
                    iter.prev_to_key(key);
                    while iter.prev_is_valid() && iter.prev_curr_key() == key{
                        iter.prev();
                    }
                }
                Bound::Unbounded=>{}
            }
            iter
        }

        pub fn next_curr_key(&self)-> Bytes{
            debug_assert!(self.next_is_valid());
            Bytes::copy_from_slice(&self.next_key)
        }

        pub fn next_curr_value(&self)->Bytes{
            debug_assert!(self.next_is_valid());
            self.block.data().slice(self.next_value_range.clone())
        }

        pub fn next_is_valid(&self) -> bool {
            !self.next_key.is_empty() && self.next_idx as isize <= self.prev_idx
        }

        pub fn prev_curr_key(&self)-> Bytes{
            debug_assert!(self.prev_is_valid());
            Bytes::copy_from_slice(&self.prev_key)
        }

        pub fn prev_curr_value(&self)->Bytes{
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

        pub fn prev(&mut self){
            self.prev_idx -= 1;
            if self.prev_idx < 0  || self.prev_idx < (self.next_idx as isize){
                self.prev_key.clear();
                self.prev_value_range = 0..0;
                return;
            }
            self.prev_to_idx(self.prev_idx);
        }

        pub fn seek_to_key(&mut self, key: &[u8]){
            match self.block.as_ref(){
                Block::Normal(block)=>{
                    let mut left = 0;
                    let mut right = block.offsets.len();
                    while left < right{
                        let mid = left + (right - left) / 2;
                        self.seek_to_idx(mid);
                        debug_assert!(self.next_is_valid());
                        if self.next_key.as_slice() == key{
                            return;
                        }
                        if self.next_key.as_slice() < key{
                            left = mid + 1;
                        }else{
                            right = mid;
                        }
                    }
                    self.seek_to_idx(left);
                }
                Block::Large(block)=>{
                    if key != block.key(){
                        self.seek_to_idx(1);
                    }
                }
            }
        }

        pub fn prev_to_key(&mut self, key: &[u8]){
            match self.block.as_ref(){
                Block::Normal(block)=>{
                    let mut left = 0;
                    let mut right = block.offsets.len();
                    while left < right{
                        let mid = left + (right - left) / 2;
                        self.prev_to_idx(mid as isize);
                        debug_assert!(self.prev_is_valid());
                        if self.prev_key.as_slice() > key{
                            right = mid;
                        }else{
                            left = mid + 1;
                        }
                    }
                    self.prev_to_idx(left as isize - 1);
                }
                Block::Large(block)=>{
                    if key != block.key(){
                        self.prev_to_idx(-1);
                    }
                }
            }
        }

        fn seek_to_idx(&mut self, idx: usize){
           match self.block.as_ref(){
                Block::Normal(block)=>{
                    if idx >= block.offsets.len(){
                        self.next_key.clear();
                        self.next_value_range = 0..0;
                        return;
                    }
                    let offset = block.offsets[idx] as usize;
                    self.seek_to_offset(offset);
                    self.next_idx = idx;
                   
                }
                Block::Large(block)=>{
                    if idx > 0{
                        self.next_key.clear();
                        self.next_value_range = 0..0;
                        return;
                    }
                    self.next_key = block.key().to_vec();
                    self.next_value_range = (SIZE_OF_U16 + block.key_length) .. block.data.len();
                    self.next_idx = idx;
                }
           }
        }

        fn prev_to_idx(&mut self, idx: isize){
            match self.block.as_ref(){
                Block::Normal(block)=>{
                    if idx < 0{
                        self.prev_key.clear();
                        self.prev_value_range = 0..0;
                        return;
                    }
                    let offset = block.offsets[idx as usize] as usize;
                    self.prev_to_offset(offset);
                    self.prev_idx = idx;
                }
                Block::Large(block)=>{
                    if idx < 0{
                        self.prev_key.clear();
                        self.prev_value_range = 0..0;
                        return;
                    }
                    self.prev_key = block.key().to_vec();
                    self.prev_value_range = (SIZE_OF_U16 + block.key_length) .. block.data.len();
                    self.prev_idx = idx;
                }
                
            }
        }

        fn seek_to_offset(&mut self, offset: usize){
            match self.block.as_ref(){
                Block::Normal(block)=>{
                    let mut rest = &block.data[offset..];
                    let common_prefix_len = rest.get_u8() as usize;
                    let key_suffix_len = rest.get_u16() as usize;
                    self.next_key.clear();
                    self.next_key.extend_from_slice(&self.first_key[..common_prefix_len]);
                    self.next_key.extend_from_slice(&rest[..key_suffix_len]);
                    rest.advance(key_suffix_len);
                    let value_len = rest.get_u16() as usize;
                    let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len + SIZE_OF_U16;
                    self.next_value_range = value_start..value_start + value_len;
                    rest.advance(value_len);
                },
                Block::Large(block)=>{
                    self.next_key = block.key().to_vec();
                    self.next_value_range = (SIZE_OF_U16 + block.key_length) .. block.data.len();
                }
            }
        }

        fn prev_to_offset(&mut self, offset: usize){
            match self.block.as_ref(){
                Block::Normal(block)=>{
                    let mut rest = &block.data[offset..];
                    let common_prefix_len = rest.get_u8() as usize;
                    let key_suffix_len = rest.get_u16() as usize;
                    self.prev_key.clear();
                    self.prev_key.extend_from_slice(&self.first_key[..common_prefix_len]);
                    self.prev_key.extend_from_slice(&rest[..key_suffix_len]);
                    rest.advance(key_suffix_len);
                    let value_len = rest.get_u16() as usize;
                    let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len + SIZE_OF_U16;
                    self.prev_value_range = value_start..value_start + value_len;
                    rest.advance(value_len);
                },
                Block::Large(block)=>{
                    self.next_key = block.key().to_vec();
                    self.next_value_range = (SIZE_OF_U16 + block.key_length) .. block.data.len();
                
                }
            }
        }
    }

    impl Iterator for BlockIter{
        type Item = (Bytes, Bytes);

        fn next(&mut self)->Option<Self::Item>{
            if !self.next_is_valid(){
                return None;
            }
            let key = self.next_curr_key();
            let value = self.next_curr_value();
            self.next();
            Some((key, value))
        }
    }

    impl DoubleEndedIterator for BlockIter{
        fn next_back(&mut self) -> Option<Self::Item> {
            if !self.prev_is_valid(){
                return None;
            }
            let key = self.prev_curr_key();
            let value = self.prev_curr_value();
            self.prev();
            Some((key, value))
        }
    }

    #[derive(Debug)]
    pub struct LargeValueBlock{
        data: Bytes,
        key_length: usize,
    }

    impl LargeValueBlock{
        fn key(&self)->Bytes{
            self.data.slice(SIZE_OF_U16..SIZE_OF_U16 + self.key_length )
        }

        fn value(&self)->Bytes{
            self.data.slice(SIZE_OF_U16 + self.key_length  ..)
        }

        fn value_length(&self)->usize{
            self.data.len() - SIZE_OF_U16 - self.key_length  - SIZE_OF_U32
        }

        /// ┌───────────────────────────────────────────────┐
        /// │Large Block                                    │
        /// │┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ┬ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ │
        /// │  key length │  key    value   Block Checksum ││
        /// ││    u16     │ bytes │ bytes │      u32        │
        /// │ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘│
        /// └───────────────────────────────────────────────┘
        fn encode(&self)->Bytes{
            let mut buf = Vec::with_capacity(self.key_length+ self.value_length() + SIZE_OF_U16 + SIZE_OF_U32);
            buf.put_u16(self.key_length as u16);
            buf.put_slice(&self.key());
            buf.put_slice(&self.value());
            let checksum = crc32fast::hash(&buf);
            buf.put_u32(checksum);
            buf.into()
        }

        fn decode(bytes:Bytes)->LoroResult<Self>{
            let key_len = (&bytes[..SIZE_OF_U16]).get_u16() as usize;
            let checksum = bytes.slice(bytes.len() - SIZE_OF_U32..).get_u32();
            if checksum != crc32fast::hash(&bytes[..bytes.len()  - SIZE_OF_U32]){
                return Err(LoroError::DecodeChecksumMismatchError);
            }
            Ok(LargeValueBlock{
                data:bytes,
                key_length: key_len,
            })
        }
    }

    #[derive(Debug)]
    pub struct NormalBlock {
        data: Bytes,
        offsets: Vec<u16>,
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
            let checksum = crc32fast::hash(&buf);
            buf.put_u32(checksum);
            buf.into()
        }

        /// 
        /// # Errors
        /// - [LoroError::DecodeChecksumMismatchError]
        fn decode(raw_block_and_check: Bytes)-> LoroResult<NormalBlock>{
            let data = raw_block_and_check.slice(..raw_block_and_check.len() - SIZE_OF_U32);
            let checksum = (&raw_block_and_check[raw_block_and_check.len() - SIZE_OF_U32..]).get_u32();
            if checksum != crc32fast::hash(data.as_ref()){
                return Err(LoroError::DecodeChecksumMismatchError);
            }
            let offsets_len = (&data[data.len()-SIZE_OF_U16..]).get_u16() as usize;
            let data_end = data.len() - SIZE_OF_U16 * (offsets_len + 1);
            let offsets = &data[data_end..data.len()-SIZE_OF_U16];
            let offsets = offsets.chunks(SIZE_OF_U16).map(|mut chunk| chunk.get_u16()).collect();
            Ok(NormalBlock{
                data: data.slice(..data_end),
                offsets,
            })
        }

        fn first_key(&self)->Bytes{
            let mut buf = self.data.as_ref();
            // skip common prefix of the first key
            buf.get_u8();
            let key_len = buf.get_u16() as usize;
            Bytes::from(buf[..key_len].to_vec())
        }
    }

    #[derive(Debug)]
    pub enum Block{
        Normal(NormalBlock),
        Large(LargeValueBlock),
    }

    impl Block{
        fn is_large(&self)->bool{
            matches!(self, Block::Large(_))
        }

        fn data(&self)->Bytes{
            match self{
                Block::Normal(block)=>block.data.clone(),
                Block::Large(block)=>block.data.clone(),
            }
        }

        fn first_key(&self)->Bytes{
            match self{
                Block::Normal(block)=>block.first_key(),
                Block::Large(block)=>block.key(),
            }
        }

        fn encode(&self)->Bytes{
            match self{
                Block::Normal(block)=>block.encode(),
                Block::Large(block)=>block.encode(),
            }
        }

        fn decode(raw_block_and_check: Bytes, is_large: bool)->LoroResult<Self>{
            if is_large{
                return LargeValueBlock::decode(raw_block_and_check).map(Block::Large);
            }
            NormalBlock::decode(raw_block_and_check).map(Block::Normal)
        }

        fn len(&self)->usize{
            match self{
                Block::Normal(block)=>block.offsets.len(),
                Block::Large(_)=>1,
            }
        }
    }

    #[derive(Debug)]
    pub struct BlockBuilder {
        data: Vec<u8>,
        offsets: Vec<u16>,
        block_size: usize,
        // for key compression
        first_key: Vec<u8>,
        is_large: bool,
    }

    impl BlockBuilder {
        pub fn new(block_size: usize) -> Self {
            Self {
                data: Vec::new(),
                offsets: Vec::new(),
                block_size,
                first_key: Vec::new(),
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
        /// ┌───────────────────────────────────────────────────────────────┐
        /// │  Key Value Chunk                                              │
        /// │┌ ─ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ┬ ─ ─ ─ ┐│
        /// │ common prefix len key suffix len│key suffix│value len  value  │
        /// ││       u8        │     u16      │  bytes   │   u16   │ bytes ││
        /// │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ │
        /// └───────────────────────────────────────────────────────────────┘
        /// 
        pub fn add(&mut self, key: &[u8], value: &[u8]) -> bool {
            debug_assert!(!key.is_empty(), "key cannot be empty");
            if  self.first_key.is_empty() && value.len() > self.block_size {
                let key_len = key.len() as u16;
                self.data.put_u16(key_len);
                self.data.put(key);
                self.data.put(value);
                self.is_large = true;
                self.first_key = key.to_vec();
                return true;
            }

            // whether the block is full
            if self.estimated_size() + key.len() + value.len() + SIZE_OF_U8 + SIZE_OF_U16 * 2 > self.block_size && !self.first_key.is_empty() {
                return false;
            }

            self.offsets.push(self.data.len() as u16);
            let (common, suffix) = get_common_prefix_len_and_strip(key, &self.first_key);
            let key_len = suffix.len() as u16;
            let value_len = value.len() as u16;
            self.data.put_u8(common);
            self.data.put_u16(key_len);
            self.data.put(suffix);
            self.data.put_u16(value_len);
            self.data.put(value);
            if self.first_key.is_empty() {
                self.first_key = key.to_vec();
            }
            true
        }

        pub fn build(self)->Block{
            if self.is_large{
                return Block::Large(LargeValueBlock{
                    data: Bytes::from(self.data),
                    key_length: self.first_key.len(),
                });
            }
            debug_assert!(!self.offsets.is_empty(), "block is empty");
            Block::Normal(NormalBlock{
                data: Bytes::from(self.data),
                offsets: self.offsets,
            })
        }
    }

    /// ┌──────────────────────────────────────────────────────────────────────────────────────┐
    /// │ Block Meta                                                                           │
    /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ┐ │
    /// │  block offset │ first key len   first key   is large │ last key len     last key     │
    /// ││     u32      │      u16      │   bytes   │    u8    │  u16(option)  │bytes(option)│ │
    /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
    /// └──────────────────────────────────────────────────────────────────────────────────────┘
    #[derive(Debug, Clone)]
    struct BlockMeta{
        offset: usize,
        is_large: bool,
        first_key: Bytes,
        last_key: Option<Bytes>,
    }

    impl BlockMeta{
        /// ┌────────────────────────────────────────────────────────────┐
        /// │ All Block Meta                                             │
        /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─┌ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ │
        /// │  block length │ Block Meta │ ...  │ Block Meta │ checksum ││
        /// ││     u32      │   bytes    │      │   bytes    │   u32     │
        /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ┘─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ┘│
        /// └────────────────────────────────────────────────────────────┘
        fn encode_meta(meta: &[BlockMeta], buf: &mut Vec<u8>){
            // the number of blocks
            let mut estimated_size = SIZE_OF_U32;
            for m in meta{
                // offset
                estimated_size += SIZE_OF_U32;
                // first key length
                estimated_size += SIZE_OF_U16;
                // first key
                estimated_size += m.first_key.len();
                // is large
                estimated_size += SIZE_OF_U8;
                if m.is_large{
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
            buf.put_u32(meta.len() as u32);
            for m in meta{
                buf.put_u32(m.offset as u32);
                buf.put_u16(m.first_key.len() as u16);
                buf.put_slice(&m.first_key);
                buf.put_u8(m.is_large as u8);
                if m.is_large{
                    continue;
                }
                buf.put_u16(m.last_key.as_ref().unwrap().len() as u16);
                buf.put_slice(m.last_key.as_ref().unwrap());
            }
            let checksum = crc32fast::hash(&buf[ori_length+4..]);
            buf.put_u32(checksum);
        }

        fn decode_meta(mut buf: &[u8])->LoroResult<Vec<BlockMeta>>{
            let num = buf.get_u32() as usize;
            let mut ans = Vec::with_capacity(num);
            let checksum = crc32fast::hash(&buf[..buf.remaining() - SIZE_OF_U32]);
            for _ in 0..num{
                let offset = buf.get_u32() as usize;
                let first_key_len = buf.get_u16() as usize;
                let first_key = buf.copy_to_bytes(first_key_len);
                let is_large = buf.get_u8() == 1;
                if is_large{
                    ans.push(BlockMeta{offset, is_large, first_key, last_key: None});
                    continue;
                }
                let last_key_len = buf.get_u16() as usize;
                let last_key = buf.copy_to_bytes(last_key_len);
                ans.push(BlockMeta{offset, is_large, first_key, last_key: Some(last_key)});
            }
            let checksum_read = buf.get_u32();
            if checksum != checksum_read{
                return Err(LoroError::DecodeChecksumMismatchError);
            }
            Ok(ans)
        }
    }

    pub(crate) struct SsTableBuilder{
        block_builder: BlockBuilder,
        first_key: Bytes,
        last_key: Bytes,
        data: Vec<u8>,
        meta: Vec<BlockMeta>,
        block_size: usize,
        // TODO: bloom filter
    }

    impl SsTableBuilder{
        pub fn new(block_size: usize)->Self{
            let mut data = Vec::with_capacity(5);
            data.put_u32(u32::from_be_bytes(MAGIC_NUMBER));
            data.put_u8(CURRENT_SCHEMA_VERSION);
            Self{
                block_builder: BlockBuilder::new(block_size),
                first_key: Bytes::new(),
                last_key: Bytes::new(),
                data,
                meta: Vec::new(),
                block_size,
            }
        }

        pub fn add(&mut self, key: Bytes, value: Bytes){
            if self.first_key.is_empty(){
                self.first_key = key.clone();
            }
            if self.block_builder.add(&key, &value){
                self.last_key = key;
                return;
            }

            self.finish();

            self.block_builder.add(&key, &value);
            self.first_key = key.clone();
            self.last_key = key;
        }

        pub fn is_empty(&self)->bool{
            self.meta.is_empty()
        }

        pub(crate) fn finish(&mut self){
            if self.block_builder.is_empty(){
                return;
            }
            let builder = std::mem::replace(&mut self.block_builder, BlockBuilder::new(self.block_size));
            let block = builder.build();
            let encoded_bytes = block.encode();
            let is_large = block.is_large();
            let meta = BlockMeta{
                offset: self.data.len(),
                is_large,
                first_key: std::mem::take(&mut self.first_key),
                last_key: if is_large{None}else{Some(std::mem::take(&mut self.last_key))} ,
            };
            self.meta.push(meta);
            self.data.extend_from_slice(&encoded_bytes);
        }

        /// ┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
        /// │ SsTable                                                                                         │
        /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┐│
        /// │  Magic Number │ Schema Version │ Block Chunk   ...  │  Block Chunk    Block Meta │ meta offset  │
        /// ││     u32      │       u8       │    bytes    │      │     bytes     │   bytes    │     u32     ││
        /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ │
        /// └─────────────────────────────────────────────────────────────────────────────────────────────────┘
        pub fn build(mut self)->SsTable{
            self.finish();
            let mut buf = self.data;
            let meta_offset = buf.len() as u32;
            BlockMeta::encode_meta(&self.meta, &mut buf);
            buf.put_u32(meta_offset);
            let first_key = self.meta.first().map(|m|m.first_key.clone()).unwrap_or_default();
            let last_key = self.meta.last().map(|m|m.last_key.clone().unwrap_or(self.meta.last().map(|m|m.first_key.clone()).unwrap_or_default())).unwrap_or_default();
            SsTable { 
                data: Bytes::from(buf),
                first_key, 
                last_key,
                meta: self.meta, 
                meta_offset: meta_offset as usize,
                block_cache: BlockCache::new(1 << 20),  // TODO: cache size
                keys: OnceCell::new(),
            }
        }
    }

    type BlockCache = quick_cache::sync::Cache<usize, Arc<Block>>;

    #[derive(Debug)]
     pub(crate) struct SsTable{
        // TODO: mmap?
        data: Bytes,
        pub(crate) first_key: Bytes,
        pub(crate) last_key: Bytes,
        meta: Vec<BlockMeta>,
        meta_offset: usize,
        block_cache: BlockCache,
        keys: OnceCell<FxHashSet<Bytes>>
    }

    impl Clone for SsTable{
        fn clone(&self)->Self{
            Self{
                data: self.data.clone(),
                first_key: self.first_key.clone(),
                last_key: self.last_key.clone(),
                meta: self.meta.clone(),
                meta_offset: self.meta_offset,
                block_cache: BlockCache::new(1 << 20),  // TODO: cache size
                keys: OnceCell::new(),
            }
        }
    }

    impl SsTable{
        pub fn export_all(&self)->Bytes{
           self.data.clone()
        }

        pub fn iter(&self)->SsTableIter{
            SsTableIter::new(self)
        }

        /// 
        /// 
        /// # Errors
        /// - [LoroError::DecodeChecksumMismatchError]
        /// - [LoroError::DecodeError]
        ///    - "Invalid magic number"
        ///    - "Invalid schema version"
        pub fn import_all(bytes: Bytes)-> LoroResult<Self>{
            let magic_number = u32::from_be_bytes((&bytes[..SIZE_OF_U32]).try_into().unwrap());
            if magic_number != u32::from_be_bytes(MAGIC_NUMBER){
                return Err(LoroError::DecodeError("Invalid magic number".into()));
            }
            let schema_version = bytes[SIZE_OF_U32];
            match schema_version{
                CURRENT_SCHEMA_VERSION => {},
                _ => return Err(LoroError::DecodeError(format!("Invalid schema version {}, 
                current support max version is {}", schema_version, CURRENT_SCHEMA_VERSION).into())),
            }
            let data_len = bytes.len();
            let meta_offset = (&bytes[data_len-SIZE_OF_U32..]).get_u32() as usize;
            let raw_meta = &bytes[meta_offset..data_len-SIZE_OF_U32];
            let meta = BlockMeta::decode_meta(raw_meta)?;
            let first_key = meta.first().map(|m|m.first_key.clone()).unwrap_or_default();
            let last_key = meta.last().map(|m|m.last_key.clone().unwrap_or(meta.last().map(|m|m.first_key.clone()).unwrap_or_default())).unwrap_or_default();
            let ans = Self { 
                data: bytes, 
                first_key,
                last_key,
                meta, 
                meta_offset ,
                block_cache: BlockCache::new(1 << 20), // TODO: cache size
                keys: OnceCell::new(),
            };
            Ok(ans)
        }

        pub fn find_block_idx(&self, key: &[u8]) -> usize {
            self.meta
                .partition_point(|meta| meta.first_key <= key)
                .saturating_sub(1)
        }

        /// 
        /// # Errors
        /// - [LoroError::DecodeChecksumMismatchError]
        fn read_block(&self, block_idx: usize)->LoroResult<Arc<Block>>{
            // TODO: cache
            let offset = self.meta[block_idx].offset;
            let offset_end = self.meta.get(block_idx+1).map_or(self.meta_offset, |m| m.offset);
            let raw_block_and_check = self.data.slice(offset..offset_end);
            let ans = Arc::new(Block::decode(raw_block_and_check, self.meta[block_idx].is_large)?);
            Ok(ans)
        }

        /// 
        /// # Errors
        /// - [LoroError::DecodeChecksumMismatchError]
        pub(crate) fn read_block_cached(&self, block_idx: usize)->LoroResult<Arc<Block>>{
            let block = self.block_cache.get_or_insert_with(&block_idx, ||self.read_block(block_idx))?;
            Ok(block)
        }

        /// 
        /// # Errors
        /// - [LoroError::DecodeChecksumMismatchError]
        pub fn contains_key(&self, key: &[u8])->LoroResult<bool>{
            if self.first_key > key || self.last_key < key{
                return Ok(false);
            }
            let idx = self.find_block_idx(key);
            let block = self.read_block_cached(idx)?;
            let block_iter = BlockIter::new_seek_to_key(block, key);
            Ok(block_iter.next_is_valid() && block_iter.next_curr_key() == key)
        }

        pub fn valid_keys(&self)->&FxHashSet<Bytes>{
            self.keys.get_or_init(||{
                let mut keys = FxHashSet::default();
                for (k, _) in self.iter(){
                    keys.insert(k);
                }
                keys
            })
        }

        pub fn data_len(&self)->usize{
            self.data.len()
        }
    }


    #[derive(Clone)]
    pub struct SsTableIter<'a>{
        table: &'a SsTable,
        next_block_iter: BlockIter,
        prev_block_iter: BlockIter,
        next_block_idx: usize,
        prev_block_idx: isize,
        next_first: bool
    }

    impl<'a> Debug for SsTableIter<'a>{
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SsTableIter")
            .field("next_block_iter", &self.next_block_iter)
            .field("prev_block_iter", &self.prev_block_iter)
            .field("next_block_idx", &self.next_block_idx)
            .field("prev_block_idx", &self.prev_block_idx)
            .field("next_first", &self.next_first)
            .finish()
        }
    }

    
    impl<'a> SsTableIter<'a>{
        fn new(table: &'a SsTable)->Self{
            let block = table.read_block_cached(0).unwrap();
            let block_iter = BlockIter::new_seek_to_first(block);
            let prev_block_idx = table.meta.len() -1;
            let prev_block_iter = {
                let prev_block = table.read_block_cached(prev_block_idx).unwrap();
                BlockIter::new_seek_to_first(prev_block)
            };

            Self{
                table,
                next_block_iter:block_iter,
                next_block_idx: 0,
                prev_block_iter,
                prev_block_idx: prev_block_idx as isize, 
                next_first:false
            }
        }

        pub fn new_scan(table: &'a SsTable, start: Bound<&[u8]>, end: Bound<&[u8]>)->Self{
            let (table_idx, mut iter, excluded) = match start{
                Bound::Included(start)=>{
                    let idx = table.find_block_idx(start);
                    let block = table.read_block_cached(idx).unwrap();
                    let iter = BlockIter::new_seek_to_key(block, start);
                    (idx, iter, None)
                },
                Bound::Excluded(start)=>{
                    let idx = table.find_block_idx(start);
                    let block = table.read_block_cached(idx).unwrap();
                    let iter = BlockIter::new_seek_to_key(block, start);
                    (idx, iter, Some(start))
                },
                Bound::Unbounded=>{
                    let block = table.read_block_cached(0).unwrap();
                    let iter = BlockIter::new_seek_to_first(block);
                    (0, iter, None)
                },
            };

            let (end_idx, end_iter, end_excluded) = match end {
                    Bound::Included(end)=>{
                        let end_idx = table.find_block_idx(end);
                        if end_idx == table_idx{
                            iter.prev_to_key(end);
                            // if the prev is invalid, the next should also be invalid
                            if !iter.prev_is_valid(){
                                iter.next();
                            }
                            (end_idx, None, None)
                        }else{
                            let block = table.read_block_cached(end_idx).unwrap();
                            let iter = BlockIter::new_prev_to_key(block, end);
                            (end_idx, Some(iter), None)
                        }
                    },
                    Bound::Excluded(end)=>{
                        let end_idx = table.find_block_idx(end);
                        if end_idx == table_idx{
                            iter.prev_to_key(end);
                            // if the prev is invalid, the next should also be invalid
                            if !iter.prev_is_valid(){
                                iter.next();
                            }
                            (end_idx, None, Some(end))
                        }else{
                            let block = table.read_block_cached(end_idx).unwrap();
                            let iter = BlockIter::new_prev_to_key(block, end);
                            (end_idx, Some(iter), Some(end))
                        }
                        
                    },
                    Bound::Unbounded=>{
                        let end_idx = table.meta.len() - 1;
                        if end_idx == table_idx{
                            (end_idx, None, None)
                        }else{
                            let block = table.read_block_cached(end_idx).unwrap();
                            let iter = BlockIter::new_seek_to_first(block);
                            (end_idx, Some(iter), None)
                        }
                    }
            };
            
            let mut ans = if let Some(end_iter) = end_iter{
                debug_assert!(end_idx > table_idx);
                SsTableIter{
                    table,
                    next_block_iter: iter,
                    next_block_idx: table_idx,
                    prev_block_iter: end_iter,
                    prev_block_idx: end_idx as isize,
                    next_first: false
                }
            }else{
                debug_assert!(end_idx == table_idx);
                SsTableIter{
                    table,
                    next_block_iter: iter.clone(),
                    next_block_idx: table_idx,
                    prev_block_iter: iter,
                    prev_block_idx: end_idx as isize,
                    next_first: true
                }
            };
            if let Some(key) = excluded {
                if ans.is_next_valid() && ans.next_key() == key{
                    ans.next();
                }
            }
            if let Some(key) = end_excluded {
                if ans.is_prev_valid() &&  ans.prev_key() == key{
                    ans.prev();
                }
            }
            ans
        }

        pub fn is_next_valid(&self)->bool{
            self.next_block_iter.next_is_valid()
        }

        pub fn next_key(&self)->Bytes{
            self.next_block_iter.next_curr_key()
        }

        pub fn next_value(&self)->Bytes{
            self.next_block_iter.next_curr_value()
        }

        pub fn is_prev_valid(&self)->bool{
            if self.next_first{
                self.next_block_iter.prev_is_valid()
            }else{
                self.prev_block_iter.prev_is_valid()
            }
        }

        pub fn prev_key(&self)->Bytes{
            if self.next_first{
                self.next_block_iter.prev_curr_key()
            }else{
                self.prev_block_iter.prev_curr_key()
            }
        }

        pub fn prev_value(&self)->Bytes{
            if self.next_first{
                self.next_block_iter.prev_curr_value()
            }else{
                self.prev_block_iter.prev_curr_value()
            }
        }

        pub fn next(&mut self){
            self.next_block_iter.next();
            while self.next_block_iter.next_is_valid() && self.next_block_iter.next_curr_value().is_empty(){
                self.next_block_iter.next();
            }
            if !self.next_block_iter.next_is_valid(){
                self.next_block_idx += 1;
                if self.next_block_idx > self.prev_block_idx as usize{
                    return;
                }
                if self.next_block_idx == self.prev_block_idx as usize && !self.next_first{
                    std::mem::swap(&mut self.next_block_iter, &mut self.prev_block_iter);
                    self.next_first = true;
                }else if self.next_block_idx < self.table.meta.len(){
                    let block = self.table.read_block_cached(self.next_block_idx).unwrap();
                    // TODO: cache
                    self.next_block_iter = BlockIter::new_seek_to_first(block);
                    while self.next_block_iter.next_is_valid() && self.next_block_iter.next_curr_value().is_empty(){
                        self.next();
                    }
                }
            }
        }

        pub fn prev(&mut self){
            let iter = if self.next_first{
                &mut self.next_block_iter
            }else{
                &mut self.prev_block_iter
            };
            iter.prev();
            while iter.prev_is_valid() && iter.prev_curr_value().is_empty(){
                iter.prev();
            }


            if !iter.prev_is_valid(){
                self.prev_block_idx -= 1;
                if self.next_block_idx > self.prev_block_idx as usize{
                    return;
                }
                if self.next_block_idx == self.prev_block_idx as usize && !self.next_first{
                    self.next_first = true;
                }else if self.prev_block_idx > 0 {
                    let block = self.table.read_block_cached(self.prev_block_idx as usize).unwrap();
                    // TODO: cache
                    self.prev_block_iter = BlockIter::new_seek_to_first(block);
                     while self.prev_block_iter.prev_is_valid() && self.prev_block_iter.next_curr_value().is_empty(){
                        self.prev();
                    }
                }
            }
        }
    }

    impl<'a> Iterator for SsTableIter<'a>{
        type Item = (Bytes, Bytes);
        fn next(&mut self) -> Option<Self::Item> {
            if !self.is_next_valid(){
                return None;
            }
            let key = self.next_key();
            let value = self.next_value();
            self.next();
            Some((key, value))
        }
    }

    impl<'a> DoubleEndedIterator for SsTableIter<'a>{
        fn next_back(&mut self) -> Option<Self::Item> {
            if !self.is_prev_valid(){
                return None;
            }
            let key = self.prev_key();
            let value = self.prev_value();
            self.prev();
            Some((key, value))
        }
    }

    #[cfg(test)]
    mod test{

    use super::*;
    use std:: sync::Arc;
    #[test]
    fn block_double_end_iter(){
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_seek_to_first(Arc::new(block));
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
    fn block_range_iter(){
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_scan(Arc::new(block), Bound::Included(b"key0"), Bound::Included(b"key4"));
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
        let mut iter = BlockIter::new_scan(Arc::new(block), Bound::Included(b"key1"), Bound::Included(b"key3"));
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
        let mut iter = BlockIter::new_scan(Arc::new(block),Bound::Included( b"key1"), Bound::Included(b"key3"));
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
    fn block_scan(){
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_scan(Arc::new(block), Bound::Excluded(b"key1"), Bound::Unbounded);
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
    fn block_double_end_iter_with_delete(){
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key4", b"");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new_seek_to_first(Arc::new(block));
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
    fn sstable_iter(){
        let mut builder = SsTableBuilder::new(10);
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
    fn sstable_iter_with_delete(){
        let mut builder = SsTableBuilder::new(10);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key4"), Bytes::new());
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key5"), Bytes::new());
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
    fn sstable_scan(){
        let mut builder = SsTableBuilder::new(10);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key4"), Bytes::new());
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key5"), Bytes::new());
        builder.add(Bytes::from_static(b"key3"), Bytes::from_static(b"value3"));
        let table = builder.build();
        let mut iter = SsTableIter::new_scan(&table, Bound::Excluded(b"key1"), Bound::Unbounded);
        let (k1, v1) = Iterator::next(&mut iter).unwrap();
        let (k2, v2) = DoubleEndedIterator::next_back(&mut iter).unwrap();
        assert_eq!(k1, Bytes::from_static(b"key2"));
        assert_eq!(v1, Bytes::from_static(b"value2"));
        assert_eq!(k2, Bytes::from_static(b"key3"));
        assert_eq!(v2, Bytes::from_static(b"value3"));
        assert!(Iterator::next(&mut iter).is_none());
        assert!(DoubleEndedIterator::next_back(&mut iter).is_none());
    }
    }
}

mod mem {
    use fxhash::FxHashSet;
    use sst_binary_format::{ BlockIter, SsTable, SsTableBuilder, SsTableIter};

    use super::*;
    use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

   #[derive(Debug, Clone)]
    pub struct MemKvStore{
        mem_table: BTreeMap<Bytes, Bytes>,
        ss_table: Option<SsTable>,
        block_size: usize,
    }

    impl Default for MemKvStore{
        fn default()->Self{
            Self::new(4*1024)
        }
    }

    impl MemKvStore{
        pub fn new(block_size: usize)->Self{
            Self{
                mem_table: BTreeMap::new(),
                ss_table: None,
                block_size
            }
        }
        
    }

    impl KvStore for MemKvStore{
        fn get(&self, key: &[u8]) -> Option<Bytes> {
           if let Some(v) = self.mem_table.get(key){
                if v.is_empty(){
                    return None;
                }
                return Some(v.clone());
            }

            if let Some(table) = &self.ss_table{
                if table.first_key > key || table.last_key < key{
                    return None;
                }

                // table.
                let idx = table.find_block_idx(key);
                let block = table.read_block_cached(idx).unwrap();
                let block_iter = BlockIter::new_seek_to_key(block, key);
                if block_iter.next_is_valid() && block_iter.next_curr_key() == key {
                    Some(block_iter.next_curr_value())
                }else{
                    None
                }
            }else{
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
            if self.mem_table.contains_key(key){
                return !self.mem_table.get(key).unwrap().is_empty();
            }
            if let Some(table) = &self.ss_table{
                return table.contains_key(key).unwrap();
            }
            false
        }
    
        fn scan(
            &self,
            start: std::ops::Bound<&[u8]>,
            end: std::ops::Bound<&[u8]>,
        ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_> {
            if let Some(table) = &self.ss_table{
                Box::new(MergeIterator::new(self.mem_table.range::<[u8], _>((start, end)).map(|(k,v)|(k.clone(), v.clone())), 
                    SsTableIter::new_scan(table, start, end)
                ))
            }else{
                Box::new(self.mem_table.range::<[u8], _>((start, end)).map(|(k,v)|(k.clone(), v.clone())))
            }
        }
    
        fn len(&self) -> usize {
            let deleted = self.mem_table.iter().filter(|(_, v)| v.is_empty()).map(|(k,_)|k.clone()).collect::<FxHashSet<Bytes>>();
            let default_keys = FxHashSet::default();
            let ss_keys = self.ss_table.as_ref().map_or(&default_keys, |table|table.valid_keys());
            let ss_len = ss_keys.difference(&self.mem_table.keys().cloned().collect()).count();
            self.mem_table.len() + ss_len - deleted.len()
        }
    
        fn size(&self) -> usize {
            self.mem_table.iter().fold(0, |acc, (k, v)| acc + k.len() + v.len()) + 
            self.ss_table.as_ref().map_or(0, |table|table.data_len())
        }

    
        fn export_all(&self) -> Bytes {
            let mut builder = SsTableBuilder::new(self.block_size);
            for (k, v) in self.scan(Bound::Unbounded, Bound::Unbounded){
                builder.add(k, v);
            }
            builder.finish();
            if builder.is_empty(){
                return Bytes::new();
            }
            builder.build().export_all()
        }
    
        fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
            if bytes.is_empty(){
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

    struct MergeIterator<'a, T>{
        a: T,
        b: SsTableIter<'a>,
        current_btree: Option<(Bytes, Bytes)>,
        current_sstable: Option<(Bytes, Bytes)>,
        back_btree: Option<(Bytes, Bytes)>,
        back_sstable: Option<(Bytes, Bytes)>,
    }

    impl<'a, T: DoubleEndedIterator<Item = (Bytes, Bytes)>> MergeIterator<'a, T>{
        fn new(mut a: T, b: SsTableIter<'a>)->Self{
            let current_btree = a.next();
            let back_btree = a.next_back();
            Self{
                a,
                b,
                current_btree,
                back_btree,
                current_sstable: None,
                back_sstable:None
            }
        }
    }

    impl<'a, T: DoubleEndedIterator<Item = (Bytes, Bytes)>> Iterator for MergeIterator<'a, T>{
        type Item = (Bytes, Bytes);
        fn next(&mut self) -> Option<Self::Item> {
            if self.current_sstable.is_none() && self.b.is_next_valid() {
                self.current_sstable = Some((self.b.next_key(), self.b.next_value()));
                self.b.next();
            }
            match (&self.current_btree, &self.current_sstable){
                (Some((btree_key,_)), Some((iter_key, _))) =>{
                    match btree_key.cmp(iter_key){
                        Ordering::Less=>{
                            self.current_btree.take().map(|kv|{
                                self.current_btree = self.a.next();
                                kv
                            })
                        }
                        Ordering::Equal=>{
                            self.current_sstable.take();
                            self.current_btree.take().map(|kv|{
                                self.current_btree = self.a.next();
                                kv
                            })
                        }
                        Ordering::Greater=>{
                            self.current_sstable.take()
                        }
                    }
                }
                (Some(_), None)=>{
                    self.current_btree.take().map(|kv|{
                        self.current_btree = self.a.next();
                        kv
                    })
                }
                (None, Some(_))=>{
                    self.current_sstable.take()
                }
                (None, None)=>None
            }
        }
    }

    impl<'a, T: DoubleEndedIterator<Item = (Bytes, Bytes)>> DoubleEndedIterator for MergeIterator<'a, T>{
        fn next_back(&mut self) -> Option<Self::Item> {
            if self.back_sstable.is_none() && self.b.is_prev_valid() {
                self.back_sstable = Some((self.b.prev_key(), self.b.prev_value()));
                self.b.next_back();
            }
            match (&self.back_btree, &self.back_sstable){
                (Some((btree_key,_)), Some((iter_key, _)))=>{
                    match btree_key.cmp(iter_key){
                        Ordering::Greater=>{
                            self.back_btree.take().map(|kv|{
                                self.back_btree = self.a.next_back();
                                kv
                            })
                        }
                        Ordering::Equal=>{
                            self.back_sstable.take();
                            self.back_btree.take().map(|kv|{
                                self.back_btree = self.a.next_back();
                                kv
                            })
                        }
                        Ordering::Less=>{
                            self.back_sstable.take()
                        }
                    }
                }
                 (Some(_), None)=>{
                    self.back_btree.take().map(|kv|{
                        self.back_btree = self.a.next_back();
                        kv
                    })
                }
                (None, Some(_))=>{
                    self.back_sstable.take()
                }
                (None, None)=>None
            }
        }
    }


}
