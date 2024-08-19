use bytes::Bytes;
use std::{
    ops::Bound,
    sync::{Arc, Mutex},
};

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
    use std::{ops::Range, sync::Arc};

    use bytes::{Buf, BufMut, Bytes};
    use loro_common::{LoroError, LoroResult};

    use crate::kv_store::get_common_prefix_len_and_strip;

    const MAGIC_NUMBER: [u8;4] = *b"LORO";
    const CURRENT_SCHEMA_VERSION: u8 = 0;
    const SIZE_OF_U8: usize = std::mem::size_of::<u8>();
    const SIZE_OF_U16: usize = std::mem::size_of::<u16>();
    const SIZE_OF_U32: usize = std::mem::size_of::<u32>();

    struct BlockIter{
        block: Arc<Block>,
        current_key: Vec<u8>,
        current_value_range: Range<usize>,
        idx: usize,
        first_key: Bytes,
    }

    impl BlockIter{
        pub fn new(block: Arc<Block>)->Self{
            let mut iter = Self{
                first_key: block.first_key(),
                block,
                current_key: Vec::new(),
                current_value_range: 0..0,
                idx: 0,
            };
            iter.seek_to_idx(0);
            iter
        }

        pub fn key(&self)-> Bytes{
            assert!(self.is_valid());
            Bytes::copy_from_slice(&self.current_key)
        }

        pub fn value(&self)->Bytes{
            assert!(self.is_valid());
            self.block.data.slice(self.current_value_range.clone())
        }

        pub fn is_valid(&self) -> bool {
            !self.current_key.is_empty()
        }

        pub fn next(&mut self) {
            self.idx += 1;
            self.seek_to_idx(self.idx);
        }

        pub fn seek_to_key(&mut self, key: &[u8]){
            let mut left = 0;
            let mut right = self.block.offsets.len();
            while left < right{
                let mid = left + (right - left) / 2;
                self.seek_to_idx(mid);
                assert!(self.is_valid());
                if self.current_key.as_slice() == key{
                    return;
                }
                if self.current_key.as_slice() < key{
                    left = mid + 1;
                }else{
                    right = mid;
                }
            }
            self.seek_to_idx(left);
        }

        fn seek_to_idx(&mut self, idx: usize){
            if idx >= self.block.offsets.len(){
                self.current_key.clear();
                self.current_value_range = 0..0;
                return;
            }
            let offset = self.block.offsets[idx] as usize;
            self.seek_to_offset(offset);
            self.idx = idx;
        }

        fn seek_to_offset(&mut self, offset: usize){
            let mut rest = &self.block.data[offset..];
            let common_prefix_len = rest.get_u8() as usize;
            let key_suffix_len = rest.get_u16() as usize;
            self.current_key.clear();
            self.current_key.extend_from_slice(&self.first_key[..common_prefix_len]);
            self.current_key.extend_from_slice(&rest[..key_suffix_len]);
            rest.advance(key_suffix_len);
            let value_len = rest.get_u16() as usize;
            let value_start = offset + SIZE_OF_U8 + SIZE_OF_U16 + key_suffix_len + SIZE_OF_U16;
            self.current_value_range = value_start..value_start + value_len;
            // rest.advance(value_len);
        }
    }

    impl Iterator for BlockIter{
        type Item = (Bytes, Bytes);

        fn next(&mut self)->Option<Self::Item>{
            if !self.is_valid(){
                return None;
            }
            let key = self.key();
            let value = self.value();
            self.next();
            Some((key, value))
        }
    }

    struct Block {
        data: Bytes,
        offsets: Vec<u16>,
    }

    impl Block {
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
        fn decode(raw_block_and_check: Bytes, block_length: usize)-> LoroResult<Block>{
            let data = raw_block_and_check.slice(..block_length);
            let checksum = (&raw_block_and_check[block_length..]).get_u32();
            if checksum != crc32fast::hash(data.as_ref()){
                return Err(LoroError::DecodeChecksumMismatchError);
            }


            let offsets_len = (&data[data.len()-SIZE_OF_U16..]).get_u16() as usize;
            let data_end = data.len() - SIZE_OF_U16 * (offsets_len + 1);
            let offsets = &data[data_end..data.len()-SIZE_OF_U16];
            let offsets = offsets.chunks(SIZE_OF_U16).map(|mut chunk| chunk.get_u16()).collect();
            Ok(Block{
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
    struct BlockBuilder {
        data: Vec<u8>,
        offsets: Vec<u16>,
        block_size: usize,
        // for key compression
        first_key: Vec<u8>,
    }

    impl BlockBuilder {
        fn new(block_size: usize) -> Self {
            Self {
                data: Vec::new(),
                offsets: Vec::new(),
                block_size,
                first_key: Vec::new(),
            }
        }

        fn estimated_size(&self) -> usize {
            // key-value pairs number
            SIZE_OF_U16 +
            // offsets 
            self.offsets.len() * SIZE_OF_U16 + 
            // key-value pairs data
            self.data.len() +
            // checksum
            SIZE_OF_U32
            
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
        /// // TODO: leb128
        fn add(&mut self, key: &[u8], value: &[u8]) -> bool {
            assert!(!key.is_empty(), "key cannot be empty");

            // whether the block is full
            if self.estimated_size() + key.len() + value.len() + SIZE_OF_U8 + SIZE_OF_U16 * 2 > self.block_size && !self.offsets.is_empty() {
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

        fn build(self)->Block{
            assert!(!self.offsets.is_empty(), "block is empty");
            Block{
                data: Bytes::from(self.data),
                offsets: self.offsets,
            }
        }
    }

    /// ┌────────────────────────────────────────────────────────────────────────┐
    /// │ Block Meta                                                             │
    /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┐│
    /// │  block offset │ first key len   first key   last key len    last key   │
    /// ││     u32      │      u16      │   bytes   │      u16      │   bytes   ││
    /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ │
    /// └────────────────────────────────────────────────────────────────────────┘
    struct BlockMeta{
        offset: usize,
        first_key: Bytes,
        last_key: Bytes,
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
                // last key length
                estimated_size += SIZE_OF_U16;
                // last key
                estimated_size += m.last_key.len();
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
                buf.put_u16(m.last_key.len() as u16);
                buf.put_slice(&m.last_key);
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
                let last_key_len = buf.get_u16() as usize;
                let last_key = buf.copy_to_bytes(last_key_len);
                ans.push(BlockMeta{offset, first_key, last_key});
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
            Self{
                block_builder: BlockBuilder::new(block_size),
                first_key: Bytes::new(),
                last_key: Bytes::new(),
                data: Vec::new(),
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

        fn finish(&mut self){
            let builder = std::mem::replace(&mut self.block_builder, BlockBuilder::new(self.block_size));
            let encoded_bytes = builder.build().encode();
            let meta = BlockMeta{
                offset: self.data.len(),
                first_key: std::mem::take(&mut self.first_key),
                last_key: std::mem::take(&mut self.last_key),
            };
            self.meta.push(meta);
            self.data.extend_from_slice(&encoded_bytes);
        }

        /// ┌─────────────────────────────────────────────────────────────────┐
        /// │ SsTable                                                         │
        /// │┌ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┐│
        /// │  Block Chunk   ...  │  Block Chunk    Block Meta │ meta offset  │
        /// ││    bytes    │      │     bytes     │   bytes    │     u32     ││
        /// │ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ │
        /// └─────────────────────────────────────────────────────────────────┘
        pub fn build(mut self)->SsTable{
            self.finish();
            let mut buf = self.data;
            let meta_offset = buf.len() as u32;
            BlockMeta::encode_meta(&self.meta, &mut buf);
            buf.put_u32(meta_offset);
            SsTable { 
                data: Bytes::from(buf),
                first_key: self.meta.first().unwrap().first_key.clone(), 
                last_key: self.meta.last().unwrap().last_key.clone(), meta: self.meta, meta_offset: meta_offset as usize}
        }
    }

     pub(crate) struct SsTable{
        // TODO: mmap?
        data: Bytes,
        first_key: Bytes,
        last_key: Bytes,
        meta: Vec<BlockMeta>,
        meta_offset: usize,
        // TODO: cache
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
            // let mut header = Bytes::split_to(&mut bytes, 5);
            // let magic_number = header.get_u32();
            // if magic_number != u32::from_be_bytes(MAGIC_NUMBER){
            //     return Err(LoroError::DecodeError("Invalid magic number".into()));
            // }
            // let schema_version = header.get_u8();
            // match schema_version{
            //     CURRENT_SCHEMA_VERSION => {},
            //     _ => return Err(LoroError::DecodeError(format!("Invalid schema version {}, 
            //     current support max version is {}", schema_version, CURRENT_SCHEMA_VERSION).into())),
            // }
            // println!("bytes len2: {}", bytes.len());
            let data_len = bytes.len();
            let meta_offset = (&bytes[data_len-SIZE_OF_U32..]).get_u32() as usize;
            let raw_meta = &bytes[meta_offset..data_len-SIZE_OF_U32];
            let meta = BlockMeta::decode_meta(raw_meta)?;
            Ok(Self { data: bytes, first_key: meta.first().unwrap().first_key.clone(), last_key: meta.last().unwrap().last_key.clone(), meta, meta_offset })
        }

        /// 
        /// # Errors
        /// - [LoroError::DecodeChecksumMismatchError]
        fn read_block(&self, block_idx: usize)->LoroResult<Block>{
            let offset = self.meta[block_idx].offset;
            let offset_end = self.meta.get(block_idx+1).map_or(self.meta_offset, |m| m.offset);
            let block_length = offset_end - offset - SIZE_OF_U32;
            let raw_block_and_check = self.data.slice(offset..offset_end);

            Block::decode(raw_block_and_check, block_length)
        }
    }

    pub struct SsTableIter<'a>{
        table: &'a SsTable,
        block_iter: BlockIter,
        block_idx: usize,
    }

    impl<'a> SsTableIter<'a>{
        fn new(table: &'a SsTable)->Self{
            let block = table.read_block(0).unwrap();
            let block_iter = BlockIter::new(Arc::new(block));
            Self{
                table,
                block_iter,
                block_idx: 0,
            }
        }

        

        fn is_valid(&self)->bool{
            self.block_iter.is_valid()
        }

        pub fn key(&self)->Bytes{
            self.block_iter.key()
        }

        pub fn value(&self)->Bytes{
            self.block_iter.value()
        }

        pub fn next(&mut self){
            self.block_iter.next();
            if !self.block_iter.is_valid(){
                self.block_idx += 1;
                if self.block_idx < self.table.meta.len(){
                    let block = self.table.read_block(self.block_idx).unwrap();
                    // TODO: cache
                    self.block_iter = BlockIter::new(Arc::new(block));
                }
            }
        }
    }

    impl<'a> Iterator for SsTableIter<'a>{
        type Item = (Bytes, Bytes);
        fn next(&mut self) -> Option<Self::Item> {
            if !self.is_valid(){
                return None;
            }
            let key = self.key();
            let value = self.value();
            self.next();
            Some((key, value))
        }
    }
}

mod mem {
    use sst_binary_format::{SsTable, SsTableBuilder};

    use super::*;
    use std::{collections::BTreeMap, sync::Arc};
    pub type MemKvStore = BTreeMap<Bytes, Bytes>;

    impl KvStore for MemKvStore {
        fn get(&self, key: &[u8]) -> Option<Bytes> {
            self.get(key).cloned()
        }

        fn set(&mut self, key: &[u8], value: Bytes) {
            self.insert(Bytes::copy_from_slice(key), value);
        }

        fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool {
            let key = Bytes::copy_from_slice(key);
            match self.get_mut(&key) {
                Some(v) => {
                    if old.as_ref() == Some(v) {
                        self.insert(key, new);
                        true
                    } else {
                        false
                    }
                }
                None => {
                    if old.is_none() {
                        self.insert(key, new);
                        true
                    } else {
                        false
                    }
                }
            }
        }

        fn remove(&mut self, key: &[u8]) {
            self.remove(key);
        }

        fn contains_key(&self, key: &[u8]) -> bool {
            self.contains_key(key)
        }

        fn scan(
            &self,
            start: Bound<&[u8]>,
            end: Bound<&[u8]>,
        ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_> {
            Box::new(
                self.range::<[u8], _>((start, end))
                    .map(|(k, v)| (k.clone(), v.clone())),
            )
        }

        fn len(&self) -> usize {
            self.len()
        }

        fn size(&self) -> usize {
            self.iter().fold(0, |acc, (k, v)| acc + k.len() + v.len())
        }

        fn export_all(&self) -> Bytes {
            let mut table = SsTableBuilder::new(4096);
            for (k, v) in self.scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded) {
                table.add(k, v);
            }
            table.build().export_all()
        }

        fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
            default_binary_format::import(self, bytes)
        }

        // fn binary_search_by(
        //     &self,
        //     start: Bound<&[u8]>,
        //     end: Bound<&[u8]>,
        //     f: CompareFn,
        // ) -> Option<(Bytes, Bytes)> {
        //     // PERF: This is super slow
        //     for (k, v) in self.range::<[u8], _>((start, end)) {
        //         match f(k, v) {
        //             std::cmp::Ordering::Equal => return Some((k.clone(), v.clone())),
        //             std::cmp::Ordering::Less => continue,
        //             std::cmp::Ordering::Greater => break,
        //         }
        //     }

        //     None
        // }

        fn clone_store(&self) -> Arc<Mutex<dyn KvStore>> {
            Arc::new(Mutex::new(self.clone()))
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn test_export_and_import_all() {
            let mut store1 = MemKvStore::default();
            store1.insert(Bytes::from("key1"), Bytes::from("value1"));
            store1.insert(Bytes::from("key2"), Bytes::from("value2"));

            let exported = store1.export_all();
            assert!(!exported.is_empty());

            let mut store2 = MemKvStore::default();
            let result = store2.import_all(exported);

            assert!(result.is_ok());
            assert_eq!(
                store2.get(&Bytes::from("key1")),
                Some(&Bytes::from("value1"))
            );
            assert_eq!(
                store2.get(&Bytes::from("key2")),
                Some(&Bytes::from("value2"))
            );
            assert_eq!(store1.len(), store2.len());
            assert_eq!(store1.size(), store2.size());
            assert_eq!(store1, store2);
        }
    }
}
