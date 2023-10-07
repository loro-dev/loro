use std::{
    fmt::Debug,
    sync::{atomic::AtomicU64, Arc},
};

use crate::Timestamp;

#[derive(Clone)]
pub struct Configure {
    pub get_time: fn() -> Timestamp,
    pub rand: Arc<dyn SecureRandomGenerator>,
}

impl Debug for Configure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Configure")
            .field("get_time", &self.get_time)
            .finish()
    }
}

pub struct DefaultRandom;

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

impl Default for Configure {
    fn default() -> Self {
        Self {
            get_time: || 0,
            rand: Arc::new(DefaultRandom),
        }
    }
}
