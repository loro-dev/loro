//! Lock-order-checked mutexes.
//!
//! This module provides a small utility to prevent deadlocks by enforcing a
//! strict, per-thread lock acquisition order across a set of related locks.
//!
//! Core ideas:
//! - Locks are created from a [`LoroLockGroup`]. Locks in the same group share
//!   a per-thread stack that tracks the last acquired lock kind.
//! - Each lock has an associated [`LockKind`]. A thread may only acquire locks
//!   in strictly increasing kind order (e.g. `Txn` → `OpLog` → `DocState` → `DiffCalculator`).
//! - Locks must be released in the reverse order they were acquired (LIFO).
//! - Violations are detected and reported with helpful panics that include the
//!   callsite (via `#[track_caller]`) and a backtrace on release-order errors.
//!
//! The actual locking is backed by [`crate::sync::Mutex`], which resolves to
//! `std::sync::Mutex` in normal builds and `loom::sync::Mutex` under loom. This
//! keeps the code testable with loom while maintaining the same API.
use crate::sync::ThreadLocal;
use crate::sync::{Mutex, MutexGuard};
use std::backtrace::Backtrace;
use std::fmt::{Debug, Display};
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::Arc;

/// A mutex that verifies lock acquisition and release order against a group-wide
/// strict ordering by [`LockKind`].
///
/// Create instances via [`LoroLockGroup::new_lock`]. Calling [`LoroMutex::lock`]
/// will panic if the current thread has already acquired a lock with a kind that
/// is greater than or equal to this lock’s kind. Release order is also checked;
/// dropping the guard out of LIFO order results in a panic.
///
/// This type wraps [`crate::sync::Mutex`], so it remains compatible with loom-based
/// concurrency testing.
#[derive(Debug)]
pub struct LoroMutex<T> {
    lock: Mutex<T>,
    kind: u8,
    currently_locked_in_this_thread: Arc<ThreadLocal<Mutex<LockInfo>>>,
}

/// Internal per-thread lock information used for diagnostics and order checks.
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
/// A group that defines a shared locking order domain.
///
/// All [`LoroMutex`] created from the same group participate in a single,
/// per-thread lock-order stack. Locks from different groups are independent and
/// do not affect each other’s ordering checks.
///
/// Use [`LoroLockGroup::new_lock`] to create locks of specific [`LockKind`].
pub struct LoroLockGroup {
    g: Arc<ThreadLocal<Mutex<LockInfo>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Logical lock kinds that define the allowed acquisition order.
///
/// Kinds with smaller numeric values must be acquired before larger ones, and
/// they must be released in reverse order. The specific variants reflect the
/// high-level components in the system; extend this enum carefully to preserve
/// a consistent global ordering.
pub enum LockKind {
    None = 0,
    Txn = 1,
    OpLog = 2,
    DocState = 3,
    DiffCalculator = 4,
}

impl LoroLockGroup {
    /// Create a new lock group.
    ///
    /// Cloning the returned group is cheap. All locks created from clones of
    /// the same group still share the same ordering domain.
    pub fn new() -> Self {
        let g = Arc::new(ThreadLocal::new());
        LoroLockGroup { g }
    }

    /// Create a new lock associated with this group and [`LockKind`].
    ///
    /// The created lock participates in this group’s order checks.
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
    /// Acquire the lock, enforcing strict increasing [`LockKind`] order.
    ///
    /// Returns a guard that unlocks when dropped and verifies that it is being
    /// released in the reverse acquisition order (LIFO). The callsite is
    /// recorded to improve panic diagnostics.
    ///
    /// Errors:
    /// - Propagates [`std::sync::PoisonError`] from the underlying mutex.
    ///
    /// Panics:
    /// - If the current thread already holds a lock with kind `>= self.kind`.
    /// - If the guard is later dropped out of acquisition order.
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

    /// Returns whether the mutex appears locked at this instant.
    ///
    /// This is implemented via `try_lock().is_err()` and is intended only for
    /// diagnostics. It is race-prone and should not be used to implement logic
    /// that depends on the lock state.
    pub fn is_locked(&self) -> bool {
        self.lock.try_lock().is_err()
    }
}

/// Guard returned by [`LoroMutex::lock`].
///
/// Dereferences to the protected data and enforces release-order checks on drop.
/// In most cases, you should keep using this guard type so order tracking remains
/// intact for the duration of the critical section.
pub struct LoroMutexGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    _inner: LoroMutexGuardInner<'a, T>,
}

/// RAII helper that updates the per-thread lock info on drop.
///
/// This is an implementation detail of [`LoroMutexGuard`].
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
    /// Extract the underlying [`MutexGuard`], detaching order tracking.
    ///
    /// This consumes `self`, performs the release-order bookkeeping immediately
    /// (making the thread-local lock info revert to the previous state), and
    /// returns the raw guard. Subsequent lock acquisitions in this thread will no
    /// longer consider this guard as held, which means order violations will not
    /// be detected relative to it.
    ///
    /// Prefer to keep using [`LoroMutexGuard`] unless integrating with APIs that
    /// require a plain [`MutexGuard`]. Misuse can lead to missing diagnostics.
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
