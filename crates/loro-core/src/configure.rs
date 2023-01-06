use std::{fmt::Debug, sync::Arc};

use crate::{change::ChangeMergeCfg, log_store::GcConfig, Timestamp};
use ring::rand::{SecureRandom, SystemRandom};

pub struct Configure {
    pub change: ChangeMergeCfg,
    pub gc: GcConfig,
    pub get_time: fn() -> Timestamp,
    pub rand: Arc<dyn SecureRandomGenerator>,
}

impl Debug for Configure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Configure")
            .field("change", &self.change)
            .field("gc", &self.gc)
            .field("get_time", &self.get_time)
            .finish()
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

impl SecureRandomGenerator for SystemRandom {
    fn fill_byte(&self, dest: &mut [u8]) {
        self.fill(dest).unwrap();
    }
}

impl Default for Configure {
    fn default() -> Self {
        Self {
            change: ChangeMergeCfg::default(),
            gc: GcConfig::default(),
            get_time: || 0,
            rand: Arc::new(SystemRandom::new()),
        }
    }
}
