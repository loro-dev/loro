pub use crate::container::richtext::config::{StyleConfig, StyleConfigMap};

#[derive(Clone)]
pub struct Configure {
    pub(crate) text_style_config: Arc<RwLock<StyleConfigMap>>,
    record_timestamp: Arc<AtomicBool>,
    merge_interval: Arc<AtomicI64>,
    /// should be larger than 0.
    /// do not use `jitter` by default
    pub(crate) tree_position_jitter: Arc<AtomicU8>,
}

impl Default for Configure {
    fn default() -> Self {
        Self {
            text_style_config: Arc::new(RwLock::new(StyleConfigMap::default_rich_text_config())),
            record_timestamp: Arc::new(AtomicBool::new(false)),
            merge_interval: Arc::new(AtomicI64::new(1000 * 1000)),
            tree_position_jitter: Arc::new(AtomicU8::new(1)),
        }
    }
}

impl Configure {
    pub fn text_style_config(&self) -> &Arc<RwLock<StyleConfigMap>> {
        &self.text_style_config
    }

    pub fn record_timestamp(&self) -> bool {
        self.record_timestamp
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_record_timestamp(&self, record: bool) {
        self.record_timestamp
            .store(record, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn merge_interval(&self) -> i64 {
        self.merge_interval
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_merge_interval(&self, interval: i64) {
        self.merge_interval
            .store(interval, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct DefaultRandom;

#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU8},
    Arc, RwLock,
};
#[cfg(test)]
static mut TEST_RANDOM: AtomicU64 = AtomicU64::new(0);

impl SecureRandomGenerator for DefaultRandom {
    fn fill_byte(&self, dest: &mut [u8]) {
        #[cfg(not(test))]
        getrandom::getrandom(dest).unwrap();

        #[cfg(test)]
        // SAFETY: this is only used in test
        unsafe {
            let bytes = TEST_RANDOM.fetch_add(1, std::sync::atomic::Ordering::Release);
            dest.copy_from_slice(&bytes.to_le_bytes());
        }
    }
}

pub trait SecureRandomGenerator: Send + Sync {
    fn fill_byte(&self, dest: &mut [u8]);
    fn next_u64(&self) -> u64 {
        let mut buf = [0u8; 8];
        self.fill_byte(&mut buf);
        u64::from_le_bytes(buf)
    }

    fn next_u32(&self) -> u32 {
        let mut buf = [0u8; 4];
        self.fill_byte(&mut buf);
        u32::from_le_bytes(buf)
    }

    fn next_i64(&self) -> i64 {
        let mut buf = [0u8; 8];
        self.fill_byte(&mut buf);
        i64::from_le_bytes(buf)
    }

    fn next_i32(&self) -> i32 {
        let mut buf = [0u8; 4];
        self.fill_byte(&mut buf);
        i32::from_le_bytes(buf)
    }
}
