use crate::{MemKvConfig, MemKvStore, SharedBlockCache};
use bytes::Bytes;
use std::ops::Bound;
use std::path::{Path, PathBuf};

/// Native out-of-core key-value store backed by temporary SSTable files in `spill_dir`.
///
/// Recent writes accumulate in an in-memory memtable until [`Self::flush`], [`Self::export_all`],
/// [`Self::import_all`], or [`Self::import_all_unchecked`] serializes them into SSTable blocks
/// written to unique `.sst` temporary files under `spill_dir`, freeing serialized table bytes
/// from RAM while continuing to serve block reads through `SharedBlockCache`.
///
/// Temporary files are unlinked when the last reference disappears. Clones share
/// immutable spilled files; each clone keeps independent write buffers and caches.
#[derive(Debug)]
pub struct FileKvStore {
    inner: MemKvStore,
    spill_dir: PathBuf,
    /// The first error since the last [`Self::take_spill_error`]: tables that
    /// failed to spill stay in memory, so no data is lost, but the caller may
    /// need to know the store is not out of core.
    spill_error: Option<String>,
}

impl Clone for FileKvStore {
    fn clone(&self) -> Self {
        let mut inner = self.inner.clone();
        inner.set_block_cache(SharedBlockCache::new(self.inner.block_cache().max_bytes()));
        Self {
            inner,
            spill_dir: self.spill_dir.clone(),
            spill_error: None,
        }
    }
}

impl FileKvStore {
    /// Creates an out-of-core store that writes temporary SSTables under `spill_dir`.
    pub fn new(spill_dir: impl Into<PathBuf>, config: MemKvConfig) -> std::io::Result<Self> {
        let spill_dir = spill_dir.into();
        std::fs::create_dir_all(&spill_dir)?;
        Ok(Self {
            inner: MemKvStore::new(config),
            spill_dir,
            spill_error: None,
        })
    }

    /// Creates a new empty `FileKvStore` sharing the same `spill_dir`, config, and block cache.
    pub fn empty_like(&self) -> Self {
        Self {
            inner: self.inner.empty_like(),
            spill_dir: self.spill_dir.clone(),
            spill_error: None,
        }
    }

    /// Returns the directory used for spilled temporary SSTable files.
    pub fn spill_dir(&self) -> &Path {
        &self.spill_dir
    }

    /// Replaces the shared decompressed block cache used by this store.
    pub fn set_block_cache(&mut self, block_cache: SharedBlockCache) {
        self.inner.set_block_cache(block_cache);
    }

    /// Returns a reference to the shared block cache used by this store.
    pub fn block_cache(&self) -> &SharedBlockCache {
        self.inner.block_cache()
    }

    /// Returns the value stored for `key`, if present.
    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        self.inner.get(key)
    }

    /// Inserts or overwrites `key` in the in-memory write buffer.
    pub fn set(&mut self, key: &[u8], value: Bytes) {
        self.inner.set(key, value)
    }

    /// Atomically updates `key` to `new` if its current value equals `old`.
    pub fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool {
        self.inner.compare_and_swap(key, old, new)
    }

    /// Marks `key` as deleted.
    pub fn remove(&mut self, key: &[u8]) {
        self.inner.remove(key)
    }

    /// Returns `true` if `key` is present with a non-tombstone value.
    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.inner.contains_key(key)
    }

    /// Scans live entries within `[start, end]` in sorted key order.
    pub fn scan(
        &self,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_> {
        self.inner.scan(start, end)
    }

    /// Returns the number of live keys currently stored.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if there are no live keys.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns total logical key/value bytes across memtable and SSTables.
    pub fn size(&self) -> usize {
        self.inner.size()
    }

    /// Approximate RAM footprint excluding `SharedBlockCache` and spilled disk payloads.
    pub fn estimated_memory_bytes(&self) -> usize {
        self.inner.estimated_memory_bytes()
    }

    /// Returns total bytes currently spilled to temporary SSTable files on disk.
    pub fn spilled_disk_bytes(&self) -> usize {
        self.inner.spilled_disk_bytes()
    }

    /// Flushes in-memory entries to a new L0 SSTable, compacts suffixes by size-tiered/count
    /// thresholds when needed, and spills new tables to disk.
    pub fn flush(&mut self) {
        self.inner.flush();
        self.spill_tables();
    }

    /// Compacts and exports all entries as serialized SSTable bytes, spilling the compacted table.
    pub fn export_all(&mut self) -> Bytes {
        let exported = self.inner.export_all();
        self.spill_tables();
        exported
    }

    /// Imports serialized SSTable bytes with full block checksum verification and spills to disk.
    pub fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
        self.inner.import_all(bytes)?;
        self.spill_tables();
        Ok(())
    }

    /// Imports trusted serialized SSTable bytes without per-block checksum verification and spills.
    pub fn import_all_unchecked(&mut self, bytes: Bytes) -> Result<(), String> {
        self.inner.import_all_unchecked(bytes)?;
        self.spill_tables();
        Ok(())
    }

    /// Returns and clears the first spill failure since the previous call.
    pub fn take_spill_error(&mut self) -> Option<String> {
        self.spill_error.take()
    }

    fn spill_tables(&mut self) {
        if let Err(err) = self.inner.spill_tables_to_dir(&self.spill_dir) {
            tracing::warn!(?err, dir = ?self.spill_dir, "failed to spill SSTable to disk");
            self.spill_error
                .get_or_insert_with(|| format!("{}: {err}", self.spill_dir.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "loro-kv-store-{prefix}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn count_sst_files(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "sst"))
            .count()
    }

    fn list_sst_files(dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|entry| entry.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "sst"))
            .collect();
        files.sort();
        files
    }

    /// Blocks read back from a spill file are re-verified: a byte changed on disk
    /// used to be decoded as if it were valid (or panic inside the decoder).
    #[test]
    fn a_corrupted_spill_file_fails_its_checksum_instead_of_returning_wrong_data() {
        let temp = TempDir::new("corrupt");
        let mut source = MemKvStore::new(MemKvConfig::default());
        for i in 0..64_u32 {
            source.set(
                format!("key-{i:04}").as_bytes(),
                Bytes::from(vec![i as u8; 1024]),
            );
        }
        let mut store = FileKvStore::new(&temp.0, MemKvConfig::default()).unwrap();
        store.import_all(source.export_all()).unwrap();
        assert!(store.spilled_disk_bytes() > 0);
        let path = std::fs::read_dir(&temp.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "sst"))
            .unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[10] ^= 0x01;
        // Rewrite in place: the store keeps its file handle open.
        {
            use std::io::{Seek, Write};
            let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.seek(std::io::SeekFrom::Start(0)).unwrap();
            file.write_all(&bytes).unwrap();
        }
        // A clone has an empty block cache, so the read goes to the file.
        let cold = store.clone();
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cold.get(b"key-0000")))
            .expect_err("a corrupted block was decoded");
        let msg = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        assert!(msg.contains("failed its checksum"), "{msg}");
        let export =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.clone().export_all()));
        assert!(
            export.is_err(),
            "export of a corrupted spill file succeeded"
        );
    }

    #[test]
    fn spilling_releases_import_buffer_and_keeps_files_private() {
        struct OwnedBytes(Vec<u8>, std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl AsRef<[u8]> for OwnedBytes {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }
        impl Drop for OwnedBytes {
            fn drop(&mut self) {
                self.1.store(true, Ordering::SeqCst);
            }
        }
        let temp = TempDir::new("owned-spill");
        let mut source = MemKvStore::new(MemKvConfig::default());
        source.set(b"large", Bytes::from(vec![7; 128 * 1024]));
        source.set(b"small", Bytes::from_static(b"value"));
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let input = Bytes::from_owner(OwnedBytes(source.export_all().to_vec(), dropped.clone()));
        let mut store = FileKvStore::new(&temp.0, MemKvConfig::default()).unwrap();
        store.import_all(input).unwrap();
        assert!(
            dropped.load(Ordering::SeqCst),
            "spill metadata retained the input allocation"
        );
        assert!(store.spilled_disk_bytes() > 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in std::fs::read_dir(&temp.0).unwrap() {
                assert_eq!(
                    file.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        assert_eq!(store.get(b"large"), Some(Bytes::from(vec![7; 128 * 1024])));
        assert_eq!(store.get(b"small").as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn file_kv_store_spills_reads_clones_and_cleans_up_on_drop() {
        let temp = TempDir::new("spill");
        let mut store =
            FileKvStore::new(&temp.0, MemKvConfig::default().should_encode_none(false)).unwrap();
        for i in 0..200_u32 {
            let k = format!("key-{i:04}");
            let v: Vec<u8> = (0..512_u32)
                .map(|j| {
                    let x = i.wrapping_mul(2_654_435_761) ^ j.wrapping_mul(374_761_393);
                    (x ^ (x >> 13) ^ (x >> 21)) as u8
                })
                .collect();
            store.set(k.as_bytes(), Bytes::from(v));
        }
        store.flush();
        assert_eq!(count_sst_files(&temp.0), 1);
        let spilled = store.spilled_disk_bytes();
        assert!(spilled > 10_000, "spilled={spilled}");
        assert!(
            store.estimated_memory_bytes() < spilled / 2,
            "estimated_memory_bytes={} spilled={spilled}",
            store.estimated_memory_bytes()
        );

        let expected_42: Vec<u8> = (0..512_u32)
            .map(|j| {
                let x = 42_u32.wrapping_mul(2_654_435_761) ^ j.wrapping_mul(374_761_393);
                (x ^ (x >> 13) ^ (x >> 21)) as u8
            })
            .collect();
        assert_eq!(
            store.get(b"key-0042"),
            Some(Bytes::from(expected_42.clone()))
        );

        let mut cloned = store.clone();
        assert_eq!(
            count_sst_files(&temp.0),
            1,
            "cloning must not copy immutable files"
        );
        drop(store);
        assert_eq!(count_sst_files(&temp.0), 1);
        assert_eq!(cloned.get(b"key-0042"), Some(Bytes::from(expected_42)));

        let exported = cloned.export_all();
        assert!(!exported.is_empty());
        drop(cloned);
        assert_eq!(count_sst_files(&temp.0), 0);
    }

    #[test]
    fn incremental_file_flush_preserves_base_sst_file_across_small_flushes() {
        let temp = TempDir::new("incremental-spill");
        let mut store =
            FileKvStore::new(&temp.0, MemKvConfig::default().should_encode_none(false)).unwrap();
        for i in 0..400_u32 {
            let k = format!("base-{i:04}");
            let v: Vec<u8> = (0..512_u32)
                .map(|j| ((i.wrapping_mul(2_654_435_761)) ^ (j.wrapping_mul(374_761_393))) as u8)
                .collect();
            store.set(k.as_bytes(), Bytes::from(v));
        }
        store.flush();
        let files_after_base = list_sst_files(&temp.0);
        assert_eq!(files_after_base.len(), 1);
        let base_sst_path = files_after_base[0].clone();

        for i in 0..16_u32 {
            store.set(
                format!("delta-{i:02}").as_bytes(),
                Bytes::from(format!("value-{i}")),
            );
            store.flush();
            let current_files = list_sst_files(&temp.0);
            assert!(
                current_files.contains(&base_sst_path),
                "small flush #{i} rewrote base spill file {base_sst_path:?}"
            );
            assert!(
                current_files.len() <= 6,
                "spill file count grew unbounded: {}",
                current_files.len()
            );
        }

        // Deletions written in later flushes continue to shadow the base file.
        store.remove(b"base-0005");
        store.flush();
        assert_eq!(store.get(b"base-0005"), None);
        assert!(!store.contains_key(b"base-0005"));
        assert!(list_sst_files(&temp.0).contains(&base_sst_path));

        let exported = store.export_all();
        assert_eq!(count_sst_files(&temp.0), 1);
        let mut reloaded = MemKvStore::new(MemKvConfig::default().should_encode_none(false));
        reloaded.import_all(exported).unwrap();
        assert_eq!(reloaded.get(b"base-0005"), None);
        assert_eq!(reloaded.len(), 415);
    }
}
