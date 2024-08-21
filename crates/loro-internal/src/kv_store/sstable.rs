use std::{ fmt::Debug, ops::Bound, sync::Arc};

use bytes::{Buf, BufMut, Bytes};
use fxhash::FxHashSet;
use loro_common::{LoroError, LoroResult};
use once_cell::sync::OnceCell;
use rle::Sliceable;

use crate::kv_store::get_common_prefix_len_and_strip;
use super::iter::BlockIter;

const MAGIC_NUMBER: [u8;4] = *b"LORO";
const CURRENT_SCHEMA_VERSION: u8 = 0;
pub const SIZE_OF_U8: usize = std::mem::size_of::<u8>();
pub const SIZE_OF_U16: usize = std::mem::size_of::<u16>();
pub const SIZE_OF_U32: usize = std::mem::size_of::<u32>();


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
        let checksum = crc32fast::hash(&buf);
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

        let checksum = crc32fast::hash(&compressed_data);
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
            data: Bytes::from(data.slice(0, data_end)),
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

    fn encode(&self)->Bytes{
        match self{
            Block::Normal(block)=>block.encode(),
            Block::Large(block)=>block.encode(),
        }
    }

    fn decode(raw_block_and_check: Bytes, is_large: bool, key: Bytes)->Self{
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
            let key_len = key.len();
            self.data.put_u16(key_len as u16);
            let start = self.data.len();
            self.data.put(key);
            self.data.put(value);
            self.is_large = true;
            self.first_key = Bytes::copy_from_slice(&self.data[start..start+key_len]);
            return true;
        }

        // whether the block is full
        if self.estimated_size() + key.len() + value.len() + SIZE_OF_U8 + SIZE_OF_U16 * 2 > self.block_size && !self.first_key.is_empty() {
            return false;
        }

        if self.first_key.is_empty() {
            self.first_key = Bytes::copy_from_slice(key);
        }
        self.offsets.push(self.data.len() as u16);
        let (common, suffix) = get_common_prefix_len_and_strip(key, &self.first_key);
        let key_len = suffix.len() ;
        let value_len = value.len();
        self.data.put_u8(common);
        self.data.put_u16(key_len as u16);
        self.data.put(suffix);
        self.data.put_u16(value_len as u16);
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

/// ┌──────────────────────────────────────────────────────────────────────────────────────┐
/// │ Block Meta                                                                           │
/// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ┐ │
/// │  block offset │ first key len   first key   is large │ last key len     last key     │
/// ││     u32      │      u16      │   bytes   │    u8    │  u16(option)  │bytes(option)│ │
/// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
/// └──────────────────────────────────────────────────────────────────────────────────────┘
#[derive(Debug, Clone)]
pub(crate) struct BlockMeta{
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

        self.finish_block();

        self.block_builder.add(&key, &value);
        self.first_key = key.clone();
        self.last_key = key;
    }

    pub fn is_empty(&self)->bool{
        self.meta.is_empty()
    }

    pub(crate) fn finish_block(&mut self){
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
        self.finish_block();
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
        Self::check_block_checksum(&meta, &bytes, meta_offset)?;
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

    fn check_block_checksum(meta: &[BlockMeta], bytes: &Bytes, meta_offset: usize)->LoroResult<()>{
        for i in 0..meta.len(){
            let offset = meta[i].offset;
            let offset_end = meta.get(i+1).map_or(meta_offset, |m| m.offset);
            let raw_block_and_check = bytes.slice(offset..offset_end);
            let checksum = raw_block_and_check.slice(raw_block_and_check.len() - SIZE_OF_U32..).get_u32();
            if checksum != crc32fast::hash(&raw_block_and_check[..raw_block_and_check.len() - SIZE_OF_U32]){
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

    pub fn find_prev_block_idx(&self, key: &[u8])->usize{
        self.meta
            .partition_point(|meta| meta.last_key.as_ref().unwrap_or(&meta.first_key) <= key).min(self.meta.len() - 1)
    }

    fn read_block(&self, block_idx: usize)->Arc<Block>{
        let offset = self.meta[block_idx].offset;
        let offset_end = self.meta.get(block_idx+1).map_or(self.meta_offset, |m| m.offset);
        let raw_block_and_check = self.data.slice(offset..offset_end);
        Arc::new(Block::decode(raw_block_and_check, self.meta[block_idx].is_large, self.meta[block_idx].first_key.clone()))
    }


    pub(crate) fn read_block_cached(&self, block_idx: usize)->Arc<Block>{
        self.block_cache.get_or_insert_with(&block_idx, || Ok::<_, LoroError>(self.read_block(block_idx))).unwrap()
    }

    pub fn contains_key(&self, key: &[u8])->bool{
        if self.first_key > key || self.last_key < key{
            return false;
        }
        let idx = self.find_block_idx(key);
        let block = self.read_block_cached(idx);
        let block_iter = BlockIter::new_seek_to_key(block, key);
        block_iter.next_is_valid() && block_iter.next_curr_key() == key
    }

    pub fn get(&self, key: &[u8])->Option<Bytes>{
        if self.first_key > key || self.last_key < key{
            return None;
        }
        let idx = self.find_block_idx(key);
        let block = self.read_block_cached(idx);
        let block_iter = BlockIter::new_seek_to_key(block, key);
        if block_iter.next_is_valid() && block_iter.next_curr_key() == key{
            return Some(block_iter.next_curr_value());
        }
        None
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
        let block = table.read_block_cached(0);
        let block_iter = BlockIter::new_seek_to_first(block);
        let prev_block_idx = table.meta.len() -1;
        let prev_block_iter = {
            let prev_block = table.read_block_cached(prev_block_idx);
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
                let block = table.read_block_cached(idx);
                let iter = BlockIter::new_seek_to_key(block, start);
                (idx, iter, None)
            },
            Bound::Excluded(start)=>{
                let idx = table.find_block_idx(start);
                let block = table.read_block_cached(idx);
                let iter = BlockIter::new_seek_to_key(block, start);
                (idx, iter, Some(start))
            },
            Bound::Unbounded=>{
                let block = table.read_block_cached(0);
                let iter = BlockIter::new_seek_to_first(block);
                (0, iter, None)
            },
        };

        let (end_idx, end_iter, end_excluded) = match end {
                Bound::Included(end)=>{
                    let end_idx = table.find_prev_block_idx(end);
                    if end_idx == table_idx{
                        iter.prev_to_key(end);
                        // if the prev is invalid, the next should also be invalid
                        if !iter.prev_is_valid(){
                            iter.next();
                        }
                        (end_idx, None, None)
                    }else{
                        let block = table.read_block_cached(end_idx);
                        let iter = BlockIter::new_prev_to_key(block, end);
                        (end_idx, Some(iter), None)
                    }
                },
                Bound::Excluded(end)=>{
                    let end_idx = table.find_prev_block_idx(end);
                    if end_idx == table_idx{
                        iter.prev_to_key(end);
                        // if the prev is invalid, the next should also be invalid
                        if !iter.prev_is_valid(){
                            iter.next();
                        }
                        (end_idx, None, Some(end))
                    }else{
                        let block = table.read_block_cached(end_idx);
                        let iter = BlockIter::new_prev_to_key(block, end);
                        (end_idx, Some(iter), Some(end))
                    }
                    
                },
                Bound::Unbounded=>{
                    let end_idx = table.meta.len() - 1;
                    if end_idx == table_idx{
                        (end_idx, None, None)
                    }else{
                        let block = table.read_block_cached(end_idx);
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
        // the current iter is empty, but has next iter. we need to skip the empty iter
        while ans.is_next_valid() && !ans.next_block_iter.next_is_valid(){
            ans.next();
        }
        if !ans.next_first{
            while ans.is_prev_valid() && !ans.prev_block_iter.prev_is_valid(){
                ans.prev();
            }   
        }
        
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

        // need to skip empty block
        if ans.is_next_valid() && !ans.next_block_iter.next_is_valid(){
            ans.next();
        }
        if ans.is_prev_valid() && !ans.prev_block_iter.prev_is_valid(){
            ans.prev();
        }
        ans
    }

    pub fn is_next_valid(&self)->bool{
        self.next_block_iter.next_is_valid() || (self.next_block_idx as isize) < self.prev_block_idx
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
            self.prev_block_iter.prev_is_valid() || (self.next_block_idx as isize) < self.prev_block_idx
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
                let block = self.table.read_block_cached(self.next_block_idx);
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
                let block = self.table.read_block_cached(self.prev_block_idx as usize);
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