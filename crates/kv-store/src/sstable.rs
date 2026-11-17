use super::block::BlockIter;
use crate::{
    block::{Block, BlockBuilder},
    compress::CompressionType,
    iter::KvIterator,
};
use bytes::{Buf, BufMut, Bytes};
use ensure_cov::*;
use loro_common::{LoroError, LoroResult};
use std::{
    fmt::Debug,
    ops::Bound,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, RwLock,
    },
};

pub(crate) const XXH_SEED: u32 = u32::from_le_bytes(*b"LORO");
const MAGIC_BYTES: [u8; 4] = *b"LORO";
const CURRENT_SCHEMA_VERSION: u8 = 0;
pub const SIZE_OF_U8: usize = std::mem::size_of::<u8>();
pub const SIZE_OF_U16: usize = std::mem::size_of::<u16>();
pub const SIZE_OF_U32: usize = std::mem::size_of::<u32>();
/// Upper bound, in decompressed bytes, for each table's block cache. This keeps hot reads
/// cheap while guaranteeing that neither a long-lived document nor a one-shot import/export
/// walk can retain its entire store decompressed.
const BLOCK_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BLOCK_NUM: u32 = 10_000_000;

/// ```log
/// ┌──────────────────────────────────────────────────────────────────────────────────────────┐
/// │ Block Meta                                                                               │
/// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ┐ │
/// │  block offset │ first key len   first key    block type  │ last key len     last key     │
/// ││     u32      │      u16      │   bytes   │      u8      │  u16(option)  │bytes(option)│ │
/// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
/// └──────────────────────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug, Clone)]
pub(crate) struct BlockMeta {
    pub(crate) offset: usize,
    pub(crate) is_large: bool,
    pub(crate) compression_type: CompressionType,
    pub(crate) first_key: Bytes,
    pub(crate) last_key: Option<Bytes>,
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
            let large_and_compress = ((m.is_large as u8) << 7) | m.compression_type as u8;
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

    fn decode_meta(mut data: Bytes) -> LoroResult<Arc<[BlockMeta]>> {
        if data.len() < SIZE_OF_U32 + SIZE_OF_U32 {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let num = data.get_u32_le();
        if num > MAX_BLOCK_NUM {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let meta_payload_len = data
            .len()
            .checked_sub(SIZE_OF_U32)
            .ok_or_else(|| LoroError::DecodeError("Invalid bytes".into()))?;
        let min_meta_entry_len = SIZE_OF_U32 + SIZE_OF_U16 + SIZE_OF_U8;
        if (num as usize)
            .checked_mul(min_meta_entry_len)
            .is_none_or(|min_len| min_len > meta_payload_len)
        {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let checksum = xxhash_rust::xxh32::xxh32(&data[..data.len() - SIZE_OF_U32], XXH_SEED);
        let mut ans = Vec::with_capacity(num as usize);
        for _ in 0..num {
            if data.len() < SIZE_OF_U32 + SIZE_OF_U16 {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            let offset = data.get_u32_le() as usize;
            let first_key_len = data.get_u16_le() as usize;
            if data.len() < first_key_len + SIZE_OF_U8 {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            // Metadata outlives the table during export and spilling. Do not pin
            // the entire input allocation with these tiny keys.
            let first_key = Bytes::copy_from_slice(&data[..first_key_len]);
            data.advance(first_key_len);
            let is_large_and_compression_type = data.get_u8();
            let is_large = is_large_and_compression_type & 0b1000_0000 != 0;
            let compression_type = is_large_and_compression_type & 0b0111_1111;
            if is_large {
                ans.push(BlockMeta {
                    offset,
                    is_large,
                    compression_type: compression_type.try_into()?,
                    first_key,
                    last_key: None,
                });
                continue;
            }
            if data.len() < SIZE_OF_U16 {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            let last_key_len = data.get_u16_le() as usize;
            if data.len() < last_key_len {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
            let last_key = Bytes::copy_from_slice(&data[..last_key_len]);
            data.advance(last_key_len);
            ans.push(BlockMeta {
                offset,
                is_large,
                compression_type: compression_type.try_into()?,
                first_key,
                last_key: Some(last_key),
            });
        }
        if data.len() < SIZE_OF_U32 {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }
        let checksum_read = data.get_u32_le();
        if checksum != checksum_read {
            return Err(LoroError::DecodeChecksumMismatchError);
        }
        Ok(Arc::from(ans))
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
    include_none: bool, // TODO: bloom filter
}

impl SsTableBuilder {
    pub fn new(block_size: usize, compression_type: CompressionType, include_none: bool) -> Self {
        Self::with_capacity(block_size, compression_type, include_none, 0)
    }

    pub fn with_capacity(
        block_size: usize,
        compression_type: CompressionType,
        include_none: bool,
        data_capacity: usize,
    ) -> Self {
        let header_size = MAGIC_BYTES.len() + SIZE_OF_U8;
        let mut data = Vec::with_capacity(data_capacity.max(header_size));
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
            include_none,
        }
    }

    pub fn add(&mut self, key: Bytes, value: Bytes) {
        if key.is_empty() {
            return;
        }

        if !self.include_none && value.is_empty() {
            return;
        }

        if self.first_key.is_empty() {
            self.first_key = key.clone();
        }

        if self.block_builder.add(&key, &value) {
            self.last_key = key;
            return;
        }

        self.finish_current_block();
        assert!(self.block_builder.add(&key, &value));
        self.first_key = key.clone();
        self.last_key = key;
    }

    pub fn is_empty(&self) -> bool {
        self.meta.is_empty() && self.block_builder.is_empty()
    }

    pub(crate) fn finish_current_block(&mut self) {
        if self.block_builder.is_empty() {
            return;
        }
        let builder =
            std::mem::replace(&mut self.block_builder, BlockBuilder::new(self.block_size));
        let block = builder.build();
        self.add_new_block_inner(&block);
    }

    pub(crate) fn add_new_block(&mut self, block: Arc<Block>) {
        let mut should_push_one_by_one =
            self.block_builder.estimated_size() + block.data().len() + block.len() * 8
                < self.block_size;
        if !should_push_one_by_one && !self.include_none {
            for (_, v) in BlockIter::new(block.clone()) {
                if v.is_empty() {
                    should_push_one_by_one = true;
                }
            }
        }

        if should_push_one_by_one {
            // data is small, push one by one
            for (k, v) in BlockIter::new(block) {
                self.add(k, v);
            }
        } else {
            self.finish_current_block();
            if self.first_key.is_empty() {
                self.first_key = block.first_key();
            }

            self.first_key = block.first_key();
            self.last_key = block.last_key();
            self.add_new_block_inner(&block);
        }
    }

    pub(crate) fn add_raw_block(&mut self, meta: &BlockMeta, raw_block_and_check: &[u8]) {
        self.finish_current_block();
        let offset = self.data.len();
        self.data.extend_from_slice(raw_block_and_check);
        self.meta.push(BlockMeta {
            offset,
            is_large: meta.is_large,
            compression_type: meta.compression_type,
            first_key: meta.first_key.clone(),
            last_key: meta.last_key.clone(),
        });
    }

    fn add_new_block_inner(&mut self, block: &Block) {
        assert!(self.block_builder.is_empty());
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

    /// ```log
    /// ┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
    /// │ SsTable                                                                                         │
    /// │┌ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─┌ ─ ─ ─ ─ ─ ─ ┐│
    /// │  Magic Number │ Schema Version │ Block Chunk   ...  │  Block Chunk    Block Meta │ meta offset  │
    /// ││     u32      │       u8       │    bytes    │      │     bytes     │   bytes    │     u32     ││
    /// │ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘─ ─ ─ ─ ─ ─ ─ │
    /// └─────────────────────────────────────────────────────────────────────────────────────────────────┘
    /// ```
    pub fn build(mut self) -> SsTable {
        self.finish_current_block();
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
        let meta: Arc<[BlockMeta]> = Arc::from(self.meta);
        let meta_bytes = calc_meta_bytes(&meta);
        SsTable {
            id: next_table_id(),
            backing: SsTableBacking::Memory(Bytes::from(buf)),
            first_key,
            last_key,
            meta,
            meta_bytes,
            meta_offset: meta_offset as usize,
            block_cache: SharedBlockCache::default_per_doc(),
        }
    }

    pub fn build_with_cache(self, block_cache: SharedBlockCache) -> SsTable {
        let mut ss = self.build();
        ss.set_block_cache(block_cache);
        ss
    }
}

static NEXT_SSTABLE_ID: AtomicU64 = AtomicU64::new(1);

fn next_table_id() -> u64 {
    NEXT_SSTABLE_ID.fetch_add(1, Ordering::Relaxed)
}

fn calc_meta_bytes(meta: &[BlockMeta]) -> usize {
    meta.iter()
        .map(|m| {
            std::mem::size_of::<BlockMeta>()
                + m.first_key.len()
                + m.last_key.as_ref().map_or(0, Bytes::len)
        })
        .sum()
}

type BlockCache = quick_cache::sync::Cache<(u64, usize), Arc<Block>, BlockDataWeighter>;

#[derive(Clone, Copy, Debug, Default)]
struct BlockDataWeighter;

impl quick_cache::Weighter<(u64, usize), Arc<Block>> for BlockDataWeighter {
    fn weight(&self, _key: &(u64, usize), block: &Arc<Block>) -> u64 {
        // Zero-weight entries are never evicted; clamp so empty blocks cannot pin themselves.
        (block.data().len() as u64).max(1)
    }
}

#[derive(Debug)]
struct SharedBlockCacheInner {
    max_bytes: AtomicUsize,
    cache: RwLock<Option<BlockCache>>,
    has_explicit_max: AtomicBool,
    has_parent: AtomicBool,
    parent: RwLock<Option<Arc<SharedBlockCacheInner>>>,
}

/// A lazily-allocated, byte-bounded block cache that can be shared across all SSTables
/// owned by a document (including both `ChangeStore` and `DocState` stores), or
/// optionally delegated to a process-wide parent cache.
#[derive(Clone, Debug)]
pub struct SharedBlockCache {
    inner: Arc<SharedBlockCacheInner>,
}

impl Default for SharedBlockCache {
    fn default() -> Self {
        Self::default_per_doc()
    }
}

impl SharedBlockCache {
    fn new_with_flag(max_bytes: usize, has_explicit_max: bool) -> Self {
        Self {
            inner: Arc::new(SharedBlockCacheInner {
                max_bytes: AtomicUsize::new(max_bytes),
                cache: RwLock::new(None),
                has_explicit_max: AtomicBool::new(has_explicit_max),
                has_parent: AtomicBool::new(false),
                parent: RwLock::new(None),
            }),
        }
    }

    /// Creates a new shared block cache bounded to at most `max_bytes` decompressed block bytes.
    /// When `max_bytes == 0`, block reads decode on demand without caching.
    pub fn new(max_bytes: usize) -> Self {
        Self::new_with_flag(max_bytes, true)
    }

    /// Creates a shared block cache with the default 4 MiB per-document budget.
    pub fn default_per_doc() -> Self {
        Self::new_with_flag(BLOCK_CACHE_MAX_BYTES as usize, false)
    }

    pub fn max_bytes(&self) -> usize {
        self.inner.max_bytes.load(Ordering::Relaxed)
    }

    pub fn used_bytes(&self) -> usize {
        let guard = self.inner.cache.read().unwrap();
        guard.as_ref().map_or(0, |c| c.weight() as usize)
    }

    pub fn clear(&self) {
        let mut guard = self.inner.cache.write().unwrap();
        *guard = None;
    }

    pub fn set_max_bytes(&self, max_bytes: usize) {
        let old = self.inner.max_bytes.swap(max_bytes, Ordering::Relaxed);
        if max_bytes == 0 || old != max_bytes {
            self.clear();
        }
    }

    /// Sets or clears an explicit per-document max cache byte cap.
    ///
    /// When `Some(bytes)`, this cache enforces its own local budget instead of delegating to
    /// an attached parent cache. When `None`, it reverts to default sizing and delegates to
    /// the parent cache if one is attached.
    pub fn set_explicit_max_bytes(&self, max_bytes: Option<usize>) {
        match max_bytes {
            Some(bytes) => {
                self.inner.has_explicit_max.store(true, Ordering::Release);
                self.set_max_bytes(bytes);
            }
            None => {
                self.inner.has_explicit_max.store(false, Ordering::Release);
                self.inner
                    .max_bytes
                    .store(BLOCK_CACHE_MAX_BYTES as usize, Ordering::Relaxed);
                self.clear();
            }
        }
    }

    /// Attaches or detaches a shared parent block cache used whenever no explicit per-document
    /// limit overrides it.
    pub fn set_parent(&self, parent: Option<&SharedBlockCache>) {
        let parent_arc = parent
            .filter(|p| !Arc::ptr_eq(&self.inner, &p.inner))
            .map(|p| p.inner.clone());
        let has_parent = parent_arc.is_some();
        {
            let mut guard = self.inner.parent.write().unwrap();
            *guard = parent_arc;
        }
        self.inner.has_parent.store(has_parent, Ordering::Release);
        if has_parent && !self.inner.has_explicit_max.load(Ordering::Acquire) {
            self.clear();
        }
    }

    #[inline]
    pub fn is_delegating_to_parent(&self) -> bool {
        self.inner.has_parent.load(Ordering::Acquire)
            && !self.inner.has_explicit_max.load(Ordering::Acquire)
    }

    /// Returns bytes cached locally in this cache, returning `0` when delegating to a parent.
    pub fn local_used_bytes(&self) -> usize {
        if self.is_delegating_to_parent() {
            0
        } else {
            self.used_bytes()
        }
    }

    pub fn get_or_insert_with(
        &self,
        table_id: u64,
        block_idx: usize,
        table_blocks_hint: usize,
        decode: impl FnOnce() -> Arc<Block>,
    ) -> Arc<Block> {
        if self.is_delegating_to_parent() {
            let parent_arc = { self.inner.parent.read().unwrap().clone() };
            if let Some(parent) = parent_arc {
                return Self::get_or_insert_on_inner(
                    &parent,
                    table_id,
                    block_idx,
                    table_blocks_hint,
                    decode,
                );
            }
        }
        Self::get_or_insert_on_inner(&self.inner, table_id, block_idx, table_blocks_hint, decode)
    }

    fn get_or_insert_on_inner(
        inner: &SharedBlockCacheInner,
        table_id: u64,
        block_idx: usize,
        table_blocks_hint: usize,
        decode: impl FnOnce() -> Arc<Block>,
    ) -> Arc<Block> {
        let max_bytes = inner.max_bytes.load(Ordering::Relaxed);
        if max_bytes == 0 {
            return decode();
        }
        {
            let guard = inner.cache.read().unwrap();
            if let Some(cache) = guard.as_ref() {
                return cache
                    .get_or_insert_with(&(table_id, block_idx), || Ok::<_, LoroError>(decode()))
                    .expect("validated SSTable block cache insert should succeed");
            }
        }
        {
            let mut guard = inner.cache.write().unwrap();
            if guard.is_none() {
                let max_blocks = (max_bytes / crate::MemKvStore::DEFAULT_BLOCK_SIZE).max(1);
                let estimated_blocks = table_blocks_hint.clamp(1, max_blocks);
                *guard = Some(BlockCache::with_weighter(
                    estimated_blocks,
                    max_bytes as u64,
                    BlockDataWeighter,
                ));
            }
            guard
                .as_ref()
                .unwrap()
                .get_or_insert_with(&(table_id, block_idx), || Ok::<_, LoroError>(decode()))
                .expect("validated SSTable block cache insert should succeed")
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
static NEXT_SPILL_FILE_ID: AtomicU64 = AtomicU64::new(1);

/// A spilled table's blocks were checksummed when the table was built or
/// imported; the file can still be changed or corrupted on disk afterwards.
/// Reads re-verify each block and fail fast rather than return wrong data
/// (`context/memory-budget.md`, "Spill IO errors").
#[cfg(not(target_arch = "wasm32"))]
const SPILLED_BLOCK_CORRUPT: &str =
    "spilled SSTable block failed its checksum: the spill file was modified or the disk is corrupt";

#[cfg(not(target_arch = "wasm32"))]
fn spilled_block_checksum_ok(raw_block_and_check: &[u8]) -> bool {
    let Some(body_len) = raw_block_and_check.len().checked_sub(SIZE_OF_U32) else {
        return false;
    };
    let expected = u32::from_le_bytes(raw_block_and_check[body_len..].try_into().unwrap());
    expected == xxhash_rust::xxh32::xxh32(&raw_block_and_check[..body_len], XXH_SEED)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct SpillFile {
    file: std::fs::File,
    path: std::path::PathBuf,
    len: usize,
    #[cfg(not(unix))]
    io_lock: std::sync::Mutex<()>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SpillFile {
    fn create_and_write(dir: &std::path::Path, bytes: &[u8]) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let pid = std::process::id();
        loop {
            let seq = NEXT_SPILL_FILE_ID.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("loro-kv-{pid}-{seq}.sst"));
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    if let Err(err) = (|| -> std::io::Result<()> {
                        use std::io::Write;
                        file.write_all(bytes)?;
                        file.flush()
                    })() {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return Err(err);
                    }
                    return Ok(Self {
                        file,
                        path,
                        len: bytes.len(),
                        #[cfg(not(unix))]
                        io_lock: std::sync::Mutex::new(()),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
    }

    fn read_slice(&self, offset: usize, len: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file.read_exact_at(&mut buf, offset as u64)?;
        }
        #[cfg(not(unix))]
        {
            use std::io::{Read, Seek, SeekFrom};
            let _guard = self.io_lock.lock().unwrap();
            (&self.file).seek(SeekFrom::Start(offset as u64))?;
            (&self.file).read_exact(&mut buf)?;
        }
        Ok(buf)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SpillFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone)]
enum SsTableBacking {
    Memory(Bytes),
    #[cfg(not(target_arch = "wasm32"))]
    File(Arc<SpillFile>),
}

#[derive(Debug)]
pub struct SsTable {
    id: u64,
    backing: SsTableBacking,
    pub(crate) first_key: Bytes,
    pub(crate) last_key: Bytes,
    pub(crate) meta: Arc<[BlockMeta]>,
    meta_bytes: usize,
    pub(crate) meta_offset: usize,
    block_cache: SharedBlockCache,
}

impl Clone for SsTable {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            backing: self.backing.clone(),
            first_key: self.first_key.clone(),
            last_key: self.last_key.clone(),
            meta: self.meta.clone(),
            meta_bytes: self.meta_bytes,
            meta_offset: self.meta_offset,
            block_cache: SharedBlockCache::new(self.block_cache.max_bytes()),
        }
    }
}

impl SsTable {
    pub fn export_all(&self) -> Bytes {
        match &self.backing {
            SsTableBacking::Memory(data) => data.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            SsTableBacking::File(file) => {
                let bytes = Bytes::from(
                    file.read_slice(0, file.len)
                        .expect("spilled SSTable file should be readable"),
                );
                Self::check_block_checksum(&self.meta, &bytes, self.meta_offset)
                    .expect(SPILLED_BLOCK_CORRUPT);
                bytes
            }
        }
    }

    pub fn set_block_cache(&mut self, block_cache: SharedBlockCache) {
        self.block_cache = block_cache;
    }

    pub fn estimated_memory_bytes(&self) -> usize {
        let backing_bytes = match &self.backing {
            SsTableBacking::Memory(data) => data.len(),
            #[cfg(not(target_arch = "wasm32"))]
            SsTableBacking::File(_) => std::mem::size_of::<SpillFile>(),
        };
        backing_bytes + self.meta_bytes + std::mem::size_of::<Self>()
    }

    pub fn spilled_disk_bytes(&self) -> usize {
        match &self.backing {
            SsTableBacking::Memory(_) => 0,
            #[cfg(not(target_arch = "wasm32"))]
            SsTableBacking::File(file) => file.len,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spill_to_dir(&mut self, dir: &std::path::Path) -> std::io::Result<()> {
        let SsTableBacking::Memory(data) = &self.backing else {
            return Ok(());
        };
        if data.is_empty() {
            return Ok(());
        }
        let file = Arc::new(SpillFile::create_and_write(dir, data)?);
        self.backing = SsTableBacking::File(file);
        Ok(())
    }

    pub fn iter(&self) -> SsTableIter<'_> {
        SsTableIter::new(self)
    }

    /// Import an SsTable, verifying every block's checksum. This is the safe
    /// default for callers that don't have an outer integrity guarantee.
    ///
    /// `validate_blocks` additionally eagerly decodes every block to validate
    /// block metadata and key ordering. The cheap structural
    /// `validate_block_ranges` check always runs regardless.
    ///
    /// # Errors
    /// - [LoroError::DecodeChecksumMismatchError]
    /// - [LoroError::DecodeError]
    ///    - "Invalid magic number"
    ///    - "Invalid schema version"
    pub fn import_all(bytes: Bytes, validate_blocks: bool) -> LoroResult<Self> {
        Self::import_all_with(
            bytes,
            validate_blocks,
            true,
            SharedBlockCache::default_per_doc(),
        )
    }

    pub(crate) fn import_all_with_cache(
        bytes: Bytes,
        validate_blocks: bool,
        check_checksum: bool,
        block_cache: SharedBlockCache,
    ) -> LoroResult<Self> {
        Self::import_all_with(bytes, validate_blocks, check_checksum, block_cache)
    }

    /// Import without per-block checksum verification (and without eager block
    /// validation).
    ///
    /// Only use this for trusted bytes produced by this crate, or after the
    /// embedded block checksums have already been validated. An outer envelope
    /// checksum alone is insufficient because it can be recomputed over malformed
    /// inner blocks.
    pub fn import_all_unchecked(bytes: Bytes) -> LoroResult<Self> {
        Self::import_all_with(bytes, false, false, SharedBlockCache::default_per_doc())
    }

    fn import_all_with(
        bytes: Bytes,
        validate_blocks: bool,
        check_checksum: bool,
        block_cache: SharedBlockCache,
    ) -> LoroResult<Self> {
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
                        "Invalid schema version {schema_version}, current support max version is {CURRENT_SCHEMA_VERSION}"
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
        let raw_meta = bytes.slice(meta_offset..data_len - SIZE_OF_U32);
        let meta = BlockMeta::decode_meta(raw_meta)?;
        Self::validate_block_ranges(&meta, meta_offset)?;
        if validate_blocks {
            Self::validate_blocks(&meta, &bytes, meta_offset)?;
        }
        if check_checksum {
            Self::check_block_checksum(&meta, &bytes, meta_offset)?;
        }
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
        let meta_bytes = calc_meta_bytes(&meta);
        let ans = Self {
            id: next_table_id(),
            backing: SsTableBacking::Memory(bytes),
            first_key,
            last_key,
            meta,
            meta_bytes,
            meta_offset,
            block_cache,
        };
        Ok(ans)
    }

    fn validate_block_ranges(meta: &[BlockMeta], meta_offset: usize) -> LoroResult<()> {
        if meta.is_empty() {
            return Err(LoroError::DecodeError("Invalid bytes".into()));
        }

        for i in 0..meta.len() {
            let offset = meta[i].offset;
            let offset_end = meta.get(i + 1).map_or(meta_offset, |m| m.offset);
            if offset < SIZE_OF_U32 + SIZE_OF_U8
                || offset_end > meta_offset
                || offset >= offset_end
                || offset_end - offset < SIZE_OF_U32
            {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }
        }

        Ok(())
    }

    fn validate_blocks(meta: &[BlockMeta], bytes: &Bytes, meta_offset: usize) -> LoroResult<()> {
        let mut last_key = None;
        for i in 0..meta.len() {
            let offset = meta[i].offset;
            let offset_end = meta.get(i + 1).map_or(meta_offset, |m| m.offset);
            let raw_block_and_check = bytes.slice(offset..offset_end);
            let block = Block::try_decode(
                raw_block_and_check,
                meta[i].is_large,
                meta[i].first_key.clone(),
                meta[i].compression_type,
            )?;

            let block_last_key = block.last_key();
            if meta[i].last_key.as_ref().unwrap_or(&meta[i].first_key) != &block_last_key {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }

            if meta[i].first_key > block_last_key {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }

            if last_key
                .as_ref()
                .is_some_and(|last_key| last_key >= &meta[i].first_key)
            {
                return Err(LoroError::DecodeError("Invalid bytes".into()));
            }

            last_key = Some(block_last_key);
        }

        Ok(())
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

    pub(crate) fn with_raw_block<R>(&self, block_idx: usize, f: impl FnOnce(&[u8]) -> R) -> R {
        let offset = self.meta[block_idx].offset;
        let offset_end = self
            .meta
            .get(block_idx + 1)
            .map_or(self.meta_offset, |m| m.offset);
        match &self.backing {
            SsTableBacking::Memory(data) => f(&data[offset..offset_end]),
            #[cfg(not(target_arch = "wasm32"))]
            SsTableBacking::File(file) => {
                let raw = file
                    .read_slice(offset, offset_end - offset)
                    .expect("spilled SSTable block should be readable");
                assert!(spilled_block_checksum_ok(&raw), "{SPILLED_BLOCK_CORRUPT}");
                f(&raw)
            }
        }
    }

    pub(crate) fn read_block(&self, block_idx: usize) -> Arc<Block> {
        let offset = self.meta[block_idx].offset;
        let offset_end = self
            .meta
            .get(block_idx + 1)
            .map_or(self.meta_offset, |m| m.offset);
        let raw_block_and_check = match &self.backing {
            SsTableBacking::Memory(data) => data.slice(offset..offset_end),
            #[cfg(not(target_arch = "wasm32"))]
            SsTableBacking::File(file) => {
                let raw = file
                    .read_slice(offset, offset_end - offset)
                    .expect("spilled SSTable block should be readable");
                assert!(spilled_block_checksum_ok(&raw), "{SPILLED_BLOCK_CORRUPT}");
                Bytes::from(raw)
            }
        };
        Arc::new(Block::decode(
            raw_block_and_check,
            self.meta[block_idx].is_large,
            self.meta[block_idx].first_key.clone(),
            self.meta[block_idx].compression_type,
        ))
    }

    pub(crate) fn read_block_cached(&self, block_idx: usize) -> Arc<Block> {
        self.block_cache
            .get_or_insert_with(self.id, block_idx, self.meta.len(), || {
                self.read_block(block_idx)
            })
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
        match &self.backing {
            SsTableBacking::Memory(data) => data.len(),
            #[cfg(not(target_arch = "wasm32"))]
            SsTableBacking::File(file) => file.len,
        }
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

impl Debug for SsTableIter<'_> {
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
        let read_block = |idx| table.read_block_cached(idx);
        let (table_idx, mut iter, excluded) = match start {
            Bound::Included(start) => {
                notify_cov("kv-store::SstableIter::new_scan::start included");
                let idx = table.find_block_idx(start);
                let block = read_block(idx);
                let iter = BlockIter::new_seek_to_key(block, start);
                (idx, iter, None)
            }
            Bound::Excluded(start) => {
                notify_cov("kv-store::SstableIter::new_scan::start excluded");
                let idx = table.find_block_idx(start);
                let block = read_block(idx);
                let iter = BlockIter::new_seek_to_key(block, start);
                (idx, iter, Some(start))
            }
            Bound::Unbounded => {
                notify_cov("kv-store::SstableIter::new_scan::start unbounded");
                let block = read_block(0);
                let iter = BlockIter::new(block);
                (0, iter, None)
            }
        };
        let (end_idx, end_iter, end_excluded) = match end {
            Bound::Included(end) => {
                notify_cov("kv-store::SstableIter::new_scan::end included");
                let end_idx = table.find_back_block_idx(end);
                if end_idx == table_idx {
                    iter.back_to_key(end);
                    // if the next back is invalid, the next should also be invalid
                    if !iter.has_next_back() {
                        iter.next();
                    }
                    (end_idx, None, None)
                } else {
                    let block = read_block(end_idx);
                    let iter = BlockIter::new_back_to_key(block, end);
                    (end_idx, Some(iter), None)
                }
            }
            Bound::Excluded(end) => {
                notify_cov("kv-store::SstableIter::new_scan::end excluded");
                let end_idx = table.find_back_block_idx(end);
                if end_idx == table_idx {
                    iter.back_to_key(end);
                    // if the next back is invalid, the next should also be invalid
                    if !iter.has_next_back() {
                        iter.next();
                    }
                    (end_idx, None, Some(end))
                } else {
                    let block = read_block(end_idx);
                    let iter = BlockIter::new_back_to_key(block, end);
                    (end_idx, Some(iter), Some(end))
                }
            }
            Bound::Unbounded => {
                notify_cov("kv-store::SstableIter::new_scan::end unbounded");
                let end_idx = table.meta.len() - 1;
                if end_idx == table_idx {
                    notify_cov("kv-store::SstableIter::new_scan::unbounded equal");
                    (end_idx, None, None)
                } else {
                    let block = read_block(end_idx);
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
            let this = &mut *self;
            this.next_block_idx += 1;
            if this.next_block_idx > this.back_block_idx as usize {
                return;
            }
            if this.next_block_idx == this.back_block_idx as usize && !this.iter.is_same() {
                this.iter.convert_back_as_same();
            } else if this.next_block_idx < this.table.meta.len() {
                let block = this.table.read_block_cached(this.next_block_idx);
                this.iter.reset_front(BlockIter::new(block));
                this.skip_next_empty();
            } else {
                unreachable!()
            }
        }
    }

    pub fn next_block(&mut self) {
        self.next_block_idx += 1;
        if self.next_block_idx > self.back_block_idx as usize {
            match &mut self.iter {
                SsTableIterInner::Same(block_iter) => block_iter.finish(),
                SsTableIterInner::Double { .. } => unreachable!(),
            }
            return;
        }
        if self.next_block_idx == self.back_block_idx as usize && !self.iter.is_same() {
            self.iter.convert_back_as_same();
        } else if self.next_block_idx < self.table.meta.len() {
            let block = self.table.read_block_cached(self.next_block_idx);
            self.iter.reset_front(BlockIter::new(block));
            self.skip_next_empty();
        } else {
            unreachable!()
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

    pub fn peek_next_block(&self) -> Option<&Arc<Block>> {
        if self.has_next() {
            Some(self.iter.front_iter().peek_block())
        } else {
            None
        }
    }
}

impl KvIterator for SsTableIter<'_> {
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

impl Iterator for SsTableIter<'_> {
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

impl DoubleEndedIterator for SsTableIter<'_> {
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
    fn imported_metadata_does_not_retain_input_buffer() {
        struct OwnedBytes(Vec<u8>, Arc<std::sync::atomic::AtomicBool>);
        impl AsRef<[u8]> for OwnedBytes {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }
        impl Drop for OwnedBytes {
            fn drop(&mut self) {
                self.1.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let mut builder = SsTableBuilder::new(4096, CompressionType::None, false);
        builder.add(Bytes::from_static(b"a"), Bytes::from_static(b"value"));
        builder.add(Bytes::from_static(b"z"), Bytes::from_static(b"value"));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let bytes = Bytes::from_owner(OwnedBytes(
            builder.build().export_all().to_vec(),
            dropped.clone(),
        ));
        let table = SsTable::import_all(bytes, false).unwrap();
        let metadata = table.meta.clone();
        drop(table);
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(metadata[0].first_key.as_ref(), b"a");
        assert_eq!(metadata[0].last_key.as_deref(), Some(b"z".as_slice()));
    }

    fn malformed_sstable_bytes(block_bytes: &[u8], meta: &[BlockMeta]) -> Bytes {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC_BYTES);
        bytes.push(CURRENT_SCHEMA_VERSION);
        bytes.extend_from_slice(block_bytes);
        let meta_offset = bytes.len();
        BlockMeta::encode_meta(meta, &mut bytes);
        bytes.extend_from_slice(&(meta_offset as u32).to_le_bytes());
        bytes.into()
    }

    fn normal_block_bytes(key: &[u8], value: &[u8]) -> Vec<u8> {
        let mut builder = BlockBuilder::new(4096);
        builder.add(key, value);
        let block = builder.build();
        let mut bytes = Vec::new();
        block.encode(&mut bytes, CompressionType::None);
        bytes
    }

    fn normal_block_bytes_from_pairs(pairs: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut builder = BlockBuilder::new(4096);
        for (key, value) in pairs {
            assert!(builder.add(key, value));
        }
        let block = builder.build();
        let mut bytes = Vec::new();
        block.encode(&mut bytes, CompressionType::None);
        bytes
    }

    fn large_block_bytes(value: &[u8]) -> Vec<u8> {
        let mut bytes = value.to_vec();
        let checksum = xxhash_rust::xxh32::xxh32(value, XXH_SEED);
        bytes.extend_from_slice(&checksum.to_le_bytes());
        bytes
    }

    #[test]
    fn block_double_end_iter() {
        let mut builder = BlockBuilder::new(4096);
        builder.add(b"key1", b"value1");
        builder.add(b"key2", b"value2");
        builder.add(b"key3", b"value3");
        let block = builder.build();
        let mut iter = BlockIter::new(Arc::new(block));
        println!("{iter:?}");
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
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4, true);
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
    fn sstable_builder_reserves_capacity_hint() {
        let builder = SsTableBuilder::with_capacity(10, CompressionType::LZ4, true, 4096);
        assert!(builder.data.capacity() >= 4096);

        let builder = SsTableBuilder::with_capacity(10, CompressionType::LZ4, true, 0);
        assert!(builder.data.capacity() >= MAGIC_BYTES.len() + SIZE_OF_U8);
    }

    #[test]
    fn sstable_iter_with_delete() {
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4, true);
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
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4, true);
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
        let mut builder = SsTableBuilder::new(10, CompressionType::LZ4, true);
        builder.add(Bytes::from_static(b"key1"), Bytes::from_static(b"value1"));
        builder.add(Bytes::from_static(b"key2"), Bytes::from_static(b"value2"));
        builder.add(Bytes::from_static(b"key3"), Bytes::from_static(b"value3"));
        let original_table = builder.build();
        let mut buffer = original_table.export_all().to_vec();
        buffer[11] = 123;
        assert!(SsTable::import_all(buffer.into(), true).is_err());
    }

    #[test]
    fn sstable_import_rejects_empty_meta() {
        assert!(SsTable::import_all(malformed_sstable_bytes(&[], &[]), false).is_err());
    }

    #[test]
    fn sstable_import_rejects_invalid_block_ranges() {
        let first_key = Bytes::from_static(b"key");
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: first_key.clone(),
            last_key: Some(first_key.clone()),
        }];
        assert!(SsTable::import_all(malformed_sstable_bytes(&[0, 1, 2], &meta), false).is_err());

        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8 + 8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: first_key.clone(),
            last_key: Some(first_key),
        }];
        assert!(SsTable::import_all(malformed_sstable_bytes(&[0, 1, 2, 3], &meta), false).is_err());
    }

    #[test]
    fn sstable_import_rejects_undecodable_block_payload() {
        let first_key = Bytes::from_static(b"key");
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: first_key.clone(),
            last_key: Some(first_key),
        }];
        let checksum_only = xxhash_rust::xxh32::xxh32(&[], XXH_SEED).to_le_bytes();
        assert!(SsTable::import_all(malformed_sstable_bytes(&checksum_only, &meta), true).is_err());
    }

    #[test]
    fn sstable_import_rejects_meta_last_key_that_mismatches_block() {
        let first_key = Bytes::from_static(b"key");
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: first_key.clone(),
            last_key: Some(Bytes::from_static(b"a")),
        }];

        assert!(SsTable::import_all(
            malformed_sstable_bytes(&normal_block_bytes(b"key", b"value"), &meta),
            true
        )
        .is_err());
    }

    #[test]
    fn sstable_import_rejects_unsorted_block_key_ranges() {
        let first_block = normal_block_bytes(b"b", b"value");
        let second_block = normal_block_bytes(b"a", b"value");
        let second_offset = SIZE_OF_U32 + SIZE_OF_U8 + first_block.len();
        let meta = [
            BlockMeta {
                offset: SIZE_OF_U32 + SIZE_OF_U8,
                is_large: false,
                compression_type: CompressionType::None,
                first_key: Bytes::from_static(b"b"),
                last_key: Some(Bytes::from_static(b"b")),
            },
            BlockMeta {
                offset: second_offset,
                is_large: false,
                compression_type: CompressionType::None,
                first_key: Bytes::from_static(b"a"),
                last_key: Some(Bytes::from_static(b"a")),
            },
        ];
        let mut block_bytes = first_block;
        block_bytes.extend_from_slice(&second_block);

        assert!(SsTable::import_all(malformed_sstable_bytes(&block_bytes, &meta), true).is_err());
    }

    #[test]
    fn sstable_import_rejects_unsorted_keys_inside_block() {
        let block_bytes =
            normal_block_bytes_from_pairs(&[(b"a", b"1"), (b"c", b"2"), (b"b", b"3")]);
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: Bytes::from_static(b"a"),
            last_key: Some(Bytes::from_static(b"b")),
        }];

        assert!(SsTable::import_all(malformed_sstable_bytes(&block_bytes, &meta), true).is_err());
    }

    #[test]
    fn sstable_import_rejects_empty_large_block_key() {
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: true,
            compression_type: CompressionType::None,
            first_key: Bytes::new(),
            last_key: None,
        }];

        assert!(SsTable::import_all(
            malformed_sstable_bytes(&large_block_bytes(b"value"), &meta),
            true
        )
        .is_err());
    }

    #[test]
    fn sstable_import_fast_path_skips_block_validation() {
        let first_key = Bytes::from_static(b"key");
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: first_key.clone(),
            last_key: Some(Bytes::from_static(b"a")),
        }];

        let bytes = malformed_sstable_bytes(&normal_block_bytes(b"key", b"value"), &meta);
        assert!(SsTable::import_all(bytes, false).is_ok());
    }

    #[test]
    fn sstable_import_block_checksum_only_skipped_when_unchecked() {
        // The public `import_all` always verifies per-block checksums (with or
        // without the heavier `validate_blocks` decode). Only the explicit
        // `import_all_unchecked` fast path skips them, and its caller is then
        // responsible for accepting only trusted or previously validated bytes.
        let first_key = Bytes::from_static(b"key");
        let mut block_bytes = normal_block_bytes(b"key", b"value");
        *block_bytes.last_mut().unwrap() ^= 0xff;
        let meta = [BlockMeta {
            offset: SIZE_OF_U32 + SIZE_OF_U8,
            is_large: false,
            compression_type: CompressionType::None,
            first_key: first_key.clone(),
            last_key: Some(first_key),
        }];

        let bytes = malformed_sstable_bytes(&block_bytes, &meta);
        // Full validation catches the corrupted checksum.
        assert!(SsTable::import_all(bytes.clone(), true).is_err());
        // The cheap per-block checksum (always on for the public API) catches it too.
        assert!(SsTable::import_all(bytes.clone(), false).is_err());
        // Only the explicit unchecked fast path skips the check.
        assert!(SsTable::import_all_unchecked(bytes).is_ok());
    }
}
