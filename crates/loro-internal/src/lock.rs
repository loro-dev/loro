use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use std::sync::{atomic::AtomicU8, Mutex};
use std::sync::{Arc, MutexGuard};
use thread_local::ThreadLocal;

#[derive(Debug)]
pub struct LoroMutex<T> {
    lock: Mutex<T>,
    kind: u8,
    currently_locked_in_this_thread: Arc<ThreadLocal<AtomicU8>>,
}

#[derive(Debug)]
pub struct LoroLockGroup {
    g: Arc<ThreadLocal<AtomicU8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockKind {
    None = 0,
    Txn = 1,
    OpLog = 2,
    DocState = 3,
    DiffCalculator = 4,
}

impl LoroLockGroup {
    pub fn new() -> Self {
        let g = Arc::new(ThreadLocal::new());
        LoroLockGroup { g }
    }

    pub fn new_lock<T>(&self, value: T, kind: LockKind) -> LoroMutex<T> {
        LoroMutex {
            lock: Mutex::new(value),
            currently_locked_in_this_thread: self.g.clone(),
            kind: kind as u8,
        }
    }
}

impl Default for LoroLockGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> LoroMutex<T> {
    pub fn lock(&self) -> Result<LoroMutexGuard<T>, std::sync::PoisonError<MutexGuard<T>>> {
        let v = self.currently_locked_in_this_thread.get_or_default();
        let cur = v.load(Ordering::SeqCst);
        if cur >= self.kind {
            panic!(
                "Locking order violation. Current lock kind: {}, Required lock kind: {}",
                cur, self.kind
            );
        }

        let ans = self.lock.lock()?;
        v.store(self.kind, Ordering::SeqCst);
        let ans = LoroMutexGuard {
            guard: ans,
            this: self,
            this_kind: self.kind,
            last_kind: cur,
        };
        Ok(ans)
    }
}

pub struct LoroMutexGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    this: &'a LoroMutex<T>,
    this_kind: u8,
    last_kind: u8,
}

impl<T> Deref for LoroMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> DerefMut for LoroMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T: Debug> std::fmt::Debug for LoroMutexGuard<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoroMutex")
            .field("data", &self.guard)
            .finish()
    }
}

impl<T> Drop for LoroMutexGuard<'_, T> {
    fn drop(&mut self) {
        let result = self
            .this
            .currently_locked_in_this_thread
            .get_or_default()
            .compare_exchange(
                self.this_kind,
                self.last_kind,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        if result.is_err() {
            panic!(
                "Locking release order violation. self.this_kind: {}, self.last_kind: {}, current: {}",
                self.this_kind, self.last_kind, self.this.currently_locked_in_this_thread.get_or_default().load(Ordering::SeqCst)
            );
        }
    }
}
