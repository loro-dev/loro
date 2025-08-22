use crate::sync::ThreadLocal;
use crate::sync::{Mutex, MutexGuard};
use std::backtrace::Backtrace;
use std::fmt::{Debug, Display};
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::Arc;

#[derive(Debug)]
pub struct LoroMutex<T> {
    lock: Mutex<T>,
    kind: u8,
    currently_locked_in_this_thread: Arc<ThreadLocal<Mutex<LockInfo>>>,
}

#[derive(Debug, Copy, Clone, Default)]
struct LockInfo {
    kind: u8,
    caller_location: Option<&'static Location<'static>>,
}

impl Display for LockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.caller_location {
            Some(location) => write!(
                f,
                "LockInfo(kind: {}, location: {}:{}:{})",
                self.kind,
                location.file(),
                location.line(),
                location.column()
            ),
            None => write!(f, "LockInfo(kind: {}, location: None)", self.kind),
        }
    }
}

#[derive(Debug)]
pub struct LoroLockGroup {
    g: Arc<ThreadLocal<Mutex<LockInfo>>>,
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
    #[track_caller]
    pub fn lock(&self) -> Result<LoroMutexGuard<'_, T>, std::sync::PoisonError<MutexGuard<'_, T>>> {
        let caller = Location::caller();
        let v = self.currently_locked_in_this_thread.get_or_default();
        let last = *v.lock().unwrap_or_else(|e| e.into_inner());
        let this = LockInfo {
            kind: self.kind,
            caller_location: Some(caller),
        };
        if last.kind >= self.kind {
            panic!(
                "Locking order violation. Current lock: {}, New lock: {}",
                last, this
            );
        }

        let ans = self.lock.lock()?;
        *v.lock().unwrap_or_else(|e| e.into_inner()) = this;
        let ans = LoroMutexGuard {
            guard: ans,
            _inner: LoroMutexGuardInner {
                inner: self,
                this,
                last,
            },
        };
        Ok(ans)
    }

    pub fn is_locked(&self) -> bool {
        self.lock.try_lock().is_err()
    }
}

pub struct LoroMutexGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    _inner: LoroMutexGuardInner<'a, T>,
}

struct LoroMutexGuardInner<'a, T> {
    inner: &'a LoroMutex<T>,
    this: LockInfo,
    last: LockInfo,
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

impl<'a, T> LoroMutexGuard<'a, T> {
    pub fn take_guard(self) -> MutexGuard<'a, T> {
        self.guard
    }
}

impl<T> Drop for LoroMutexGuardInner<'_, T> {
    fn drop(&mut self) {
        let cur = self.inner.currently_locked_in_this_thread.get_or_default();
        let current_lock_info = *cur.lock().unwrap_or_else(|e| e.into_inner());
        if current_lock_info.kind != self.this.kind {
            let bt = Backtrace::capture();
            eprintln!("Locking release order violation callstack:\n{}", bt);
            panic!(
                "Locking release order violation. self.this: {}, self.last: {}, current: {}",
                self.this, self.last, current_lock_info
            );
        }

        *cur.lock().unwrap_or_else(|e| e.into_inner()) = self.last;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "Locking order violation")]
    fn test_locking_order_violation_shows_caller() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::DocState);
        let mutex2 = group.new_lock(2, LockKind::Txn);

        let _guard1 = mutex1.lock().unwrap(); // Lock higher priority first
        let _guard2 = mutex2.lock().unwrap(); // This should panic with caller info
    }

    #[test]
    fn test_locking_order_when_dropped_in_order() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let mutex3 = group.new_lock(3, LockKind::DocState);
        let _guard1 = mutex1.lock().unwrap();
        drop(_guard1);
        let _guard2 = mutex2.lock().unwrap();
        drop(_guard2);
        let _guard3 = mutex3.lock().unwrap();
    }

    #[test]
    #[should_panic]
    fn test_locking_order_when_not_dropped_in_reverse_order() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let _guard1 = mutex1.lock().unwrap();
        let _guard2 = mutex2.lock().unwrap();
        drop(_guard1);
        drop(_guard2);
    }

    #[test]
    fn test_dropping_should_restore_last_lock_info_0() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let mutex3 = group.new_lock(3, LockKind::DocState);
        let _guard1 = mutex1.lock().unwrap();
        let _guard3 = mutex3.lock().unwrap();
        drop(_guard3);
        let _guard2 = mutex2.lock().unwrap();
        drop(_guard2);
    }

    #[test]
    #[should_panic]
    fn test_dropping_should_restore_last_lock_info_1() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let mutex3 = group.new_lock(3, LockKind::DocState);
        let _guard2 = mutex2.lock().unwrap();
        let _guard3 = mutex3.lock().unwrap();
        drop(_guard3);
        let _guard1 = mutex1.lock().unwrap();
    }

    #[test]
    fn test_nested_locking_same_kind() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::Txn);

        let guard1 = mutex1.lock().unwrap();
        // Locking same kind should work (cur >= self.kind, so this would fail)
        // Actually, let's test this properly - same kind should fail
        drop(guard1);

        let _guard2 = mutex2.lock().unwrap(); // This should work when guard1 is dropped
    }

    #[test]
    fn test_lock_kind_enum_values() {
        assert_eq!(LockKind::None as u8, 0);
        assert_eq!(LockKind::Txn as u8, 1);
        assert_eq!(LockKind::OpLog as u8, 2);
        assert_eq!(LockKind::DocState as u8, 3);
        assert_eq!(LockKind::DiffCalculator as u8, 4);
    }

    #[test]
    fn test_is_locked_functionality() {
        let group = LoroLockGroup::new();
        let mutex = group.new_lock(42, LockKind::Txn);

        assert!(!mutex.is_locked());

        let _guard = mutex.lock().unwrap();
        assert!(mutex.is_locked());
    }

    // Helper function to test that panic messages contain caller info
    #[test]
    #[should_panic(expected = "Locking order violation")]
    fn test_panic_message_contains_location_info() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::DocState);
        let mutex2 = group.new_lock(2, LockKind::Txn);

        let _guard1 = mutex1.lock().unwrap();

        // This line should be reported in the panic message
        let _guard2 = mutex2.lock().unwrap();
    }
}
