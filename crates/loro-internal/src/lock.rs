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
//!
//! The per-thread order tracking is debug-only; in release builds those fields
//! and helpers are unused, hence the release-only `allow(dead_code)`.
#![cfg_attr(not(debug_assertions), allow(dead_code))]
use crate::sync::ThreadLocal;
use crate::sync::{Mutex, MutexGuard};
#[cfg(debug_assertions)]
use std::backtrace::Backtrace;
use std::fmt::{Debug, Display};
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// A mutex that verifies lock acquisition and release order against a group-wide
/// strict ordering by [`LockKind`].
///
/// Create instances via [`LoroLockGroup::new_lock`]. Calling [`LoroMutex::lock`]
/// will panic if the current thread has already acquired a lock with a kind that
/// is greater than or equal to this lock’s kind, or if the underlying mutex was
/// poisoned by an earlier panic. Release order is also checked; dropping the
/// guard out of LIFO order results in a panic.
///
/// This type wraps [`crate::sync::Mutex`], so it remains compatible with loom-based
/// concurrency testing.
#[derive(Debug)]
pub struct LoroMutex<T> {
    lock: Mutex<T>,
    kind: u8,
    currently_locked_in_this_thread: Arc<ThreadLocal<Mutex<LockInfo>>>,
    /// Used only by the `Txn` lock. Stored inline so the writer check reads the
    /// same allocation (usually the same cache line) as the mutex itself.
    txn_gate: TxnGate,
}

/// Keeps other threads' transaction writers out while one thread runs a
/// compound operation (undo/redo) that releases and reacquires the
/// transaction lock. Ordinary writers only read `active` after taking the
/// mutex, so the common uncontended path costs one atomic load, not a lock.
#[derive(Debug)]
struct TxnGate {
    /// True exactly while some thread holds `exclusive`. A plain std atomic
    /// even under loom: it never blocks, and modelling it would add scheduling
    /// branches to every transaction lock in every loom test. The blocking
    /// parts (`exclusive`, `owner`) stay loom-aware.
    active: std::sync::atomic::AtomicBool,
    /// Held by the owner for the whole compound operation; other compound
    /// operations and waiting writers block on it.
    exclusive: Mutex<()>,
    /// The owning thread and its reentrancy depth.
    owner: Mutex<Option<(crate::sync::thread::ThreadId, usize)>>,
}

impl TxnGate {
    fn new() -> Self {
        Self {
            active: std::sync::atomic::AtomicBool::new(false),
            exclusive: Mutex::new(()),
            owner: Mutex::new(None),
        }
    }

    // The gate guards no data and `owner` is reset by `TxnExclusiveGuard::drop`
    // during unwinding, so a panic inside an undo must not poison later writers.
    fn owner(&self) -> MutexGuard<'_, Option<(crate::sync::thread::ThreadId, usize)>> {
        self.owner.lock_ignoring_poison()
    }

    fn wait_exclusive(&self) -> MutexGuard<'_, ()> {
        self.exclusive.lock_ignoring_poison()
    }

    fn is_owned_by_current_thread(&self) -> bool {
        let me = crate::sync::thread::current().id();
        matches!(*self.owner(), Some((owner, _)) if owner == me)
    }

    /// Whether a transaction writer on this thread must wait for another
    /// thread's compound operation.
    fn must_wait(&self) -> bool {
        self.active.load(Ordering::SeqCst) && !self.is_owned_by_current_thread()
    }
}

/// Guard returned by [`LoroMutex::lock_exclusive`].
pub(crate) struct TxnExclusiveGuard<'a> {
    gate: &'a TxnGate,
    /// Only the outermost guard of a thread holds the gate.
    held: Option<MutexGuard<'a, ()>>,
}

impl Drop for TxnExclusiveGuard<'_> {
    fn drop(&mut self) {
        let mut owner = self.gate.owner();
        if let Some((_, depth)) = owner.as_mut() {
            *depth -= 1;
        }
        if self.held.is_some() {
            debug_assert!(matches!(*owner, Some((_, 0))));
            *owner = None;
            // Clear the flag before releasing the gate: waiting writers retry
            // after acquiring the gate and must then see it cleared.
            self.gate.active.store(false, Ordering::SeqCst);
        }
        drop(owner);
        self.held.take();
    }
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
            txn_gate: TxnGate::new(),
        }
    }
}

impl Default for LoroLockGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> LoroMutex<T> {
    /// Keep other threads' document writers out across a compound operation
    /// that releases and reacquires the transaction lock. The owning thread
    /// (including nested calls) keeps using the normal transaction lock.
    ///
    /// Must be called before the owner first takes the transaction lock in the
    /// operation, so every other writer either finishes before that point or
    /// observes `active` after acquiring the transaction mutex.
    pub(crate) fn lock_exclusive(&self) -> TxnExclusiveGuard<'_> {
        debug_assert_eq!(self.kind, LockKind::Txn as u8);
        let gate = &self.txn_gate;
        let me = crate::sync::thread::current().id();
        {
            let mut owner = gate.owner();
            if let Some((id, depth)) = owner.as_mut() {
                if *id == me {
                    *depth += 1;
                    return TxnExclusiveGuard { gate, held: None };
                }
            }
        }
        let held = gate.wait_exclusive();
        *gate.owner() = Some((me, 1));
        gate.active.store(true, Ordering::SeqCst);
        TxnExclusiveGuard {
            gate,
            held: Some(held),
        }
    }

    /// Acquire the underlying mutex. Transaction writers wait while another
    /// thread runs a compound operation (see [`Self::lock_exclusive`]).
    #[inline]
    fn acquire(&self) -> MutexGuard<'_, T> {
        let guard = self.lock.lock_with_kind("LoroMutex");
        if self.kind != LockKind::Txn as u8 || !self.txn_gate.must_wait() {
            return guard;
        }
        self.acquire_after_exclusive(guard)
    }

    /// Slow path: another thread owns the compound operation. Checking only
    /// after taking the mutex is enough: the owner sets `active` before its
    /// first transaction lock, so a writer either finishes before that point
    /// or sees the flag here.
    #[cold]
    fn acquire_after_exclusive<'a>(&'a self, mut guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        while self.txn_gate.must_wait() {
            drop(guard);
            drop(self.txn_gate.wait_exclusive());
            guard = self.lock.lock_with_kind("LoroMutex");
        }
        guard
    }

    #[track_caller]
    /// Acquire the lock, enforcing strict increasing [`LockKind`] order.
    ///
    /// Returns a guard that unlocks when dropped and verifies that it is being
    /// released in the reverse acquisition order (LIFO). The callsite is
    /// recorded to improve panic diagnostics.
    ///
    /// Panics:
    /// - If the underlying mutex was poisoned by an earlier panic.
    /// - If the current thread already holds a lock with kind `>= self.kind`.
    /// - If the guard is later dropped out of acquisition order.
    pub fn lock(&self) -> LoroMutexGuard<'_, T> {
        // Lock-order tracking is a debug-only deadlock-prevention aid. In release
        // builds it is compiled out entirely: per-op locking is on the hot path
        // (each local op takes the OpLog + DocState locks) and the per-thread
        // bookkeeping (a ThreadLocal lookup plus an inner mutex acquired several
        // times per lock/unlock) was measured at ~30% of B4 local-edit time.
        #[cfg(debug_assertions)]
        {
            let caller = Location::caller();
            let v = self.currently_locked_in_this_thread.get_or_default();
            let last = *v.lock();
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

            let guard = self.acquire();
            *v.lock() = this;
            LoroMutexGuard {
                guard,
                _inner: LoroMutexGuardInner {
                    inner: self,
                    this,
                    last,
                },
            }
        }
        #[cfg(not(debug_assertions))]
        {
            let guard = self.acquire();
            LoroMutexGuard {
                guard,
                _inner: LoroMutexGuardInner { inner: self },
            }
        }
    }

    /// Returns whether the mutex appears locked at this instant.
    ///
    /// This is implemented via `try_lock().is_err()` and is intended only for
    /// diagnostics. It is race-prone and should not be used to implement logic
    /// that depends on the lock state.
    pub fn is_locked(&self) -> bool {
        self.lock.is_locked()
    }

    /// Returns whether acquiring this lock would satisfy the current thread's
    /// lock-order constraints.
    ///
    /// This only checks the order tracker for the current thread. It does not
    /// guarantee that acquiring the underlying mutex will be non-blocking.
    pub(crate) fn can_lock_in_this_thread(&self) -> bool {
        #[cfg(debug_assertions)]
        {
            let v = self.currently_locked_in_this_thread.get_or_default();
            let last = *v.lock();
            last.kind < self.kind
        }
        // Without lock-order tracking (release) we cannot tell whether acquiring
        // this lock would be reentrant/out-of-order, so we conservatively report
        // "no" and let callers use their lock-free fallback (e.g. the cached
        // `visible_op_count`, which is kept exact incrementally).
        #[cfg(not(debug_assertions))]
        {
            false
        }
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
/// This is an implementation detail of [`LoroMutexGuard`]. The order-tracking
/// fields exist only in debug builds; in release the inner is a thin wrapper.
struct LoroMutexGuardInner<'a, T> {
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    inner: &'a LoroMutex<T>,
    #[cfg(debug_assertions)]
    this: LockInfo,
    #[cfg(debug_assertions)]
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
    #[cfg(not(debug_assertions))]
    fn drop(&mut self) {}

    #[cfg(debug_assertions)]
    fn drop(&mut self) {
        let cur = self.inner.currently_locked_in_this_thread.get_or_default();
        let current_lock_info = *cur.lock();
        if current_lock_info.kind != self.this.kind {
            let bt = Backtrace::capture();
            eprintln!("Locking release order violation callstack:\n{}", bt);
            panic!(
                "Locking release order violation. self.this: {}, self.last: {}, current: {}",
                self.this, self.last, current_lock_info
            );
        }

        *cur.lock() = self.last;
    }
}

// These tests exercise the lock-order instrumentation, which is only compiled
// in debug builds. Skip them under `cargo test --release`.
#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "Locking order violation")]
    fn test_locking_order_violation_shows_caller() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::DocState);
        let mutex2 = group.new_lock(2, LockKind::Txn);

        let _guard1 = mutex1.lock(); // Lock higher priority first
        let _guard2 = mutex2.lock(); // This should panic with caller info
    }

    #[test]
    fn test_locking_order_when_dropped_in_order() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let mutex3 = group.new_lock(3, LockKind::DocState);
        let _guard1 = mutex1.lock();
        drop(_guard1);
        let _guard2 = mutex2.lock();
        drop(_guard2);
        let _guard3 = mutex3.lock();
    }

    #[test]
    #[should_panic]
    fn test_locking_order_when_not_dropped_in_reverse_order() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let _guard1 = mutex1.lock();
        let _guard2 = mutex2.lock();
        drop(_guard1);
        drop(_guard2);
    }

    #[test]
    fn test_dropping_should_restore_last_lock_info_0() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let mutex3 = group.new_lock(3, LockKind::DocState);
        let _guard1 = mutex1.lock();
        let _guard3 = mutex3.lock();
        drop(_guard3);
        let _guard2 = mutex2.lock();
        drop(_guard2);
    }

    #[test]
    #[should_panic]
    fn test_dropping_should_restore_last_lock_info_1() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::OpLog);
        let mutex3 = group.new_lock(3, LockKind::DocState);
        let _guard2 = mutex2.lock();
        let _guard3 = mutex3.lock();
        drop(_guard3);
        let _guard1 = mutex1.lock();
    }

    #[test]
    fn test_nested_locking_same_kind() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::Txn);
        let mutex2 = group.new_lock(2, LockKind::Txn);

        let guard1 = mutex1.lock();
        // Locking same kind should work (cur >= self.kind, so this would fail)
        // Actually, let's test this properly - same kind should fail
        drop(guard1);

        let _guard2 = mutex2.lock(); // This should work when guard1 is dropped
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

        let _guard = mutex.lock();
        assert!(mutex.is_locked());
    }

    // Helper function to test that panic messages contain caller info
    #[test]
    #[should_panic(expected = "Locking order violation")]
    fn test_panic_message_contains_location_info() {
        let group = LoroLockGroup::new();
        let mutex1 = group.new_lock(1, LockKind::DocState);
        let mutex2 = group.new_lock(2, LockKind::Txn);

        let _guard1 = mutex1.lock();

        // This line should be reported in the panic message
        let _guard2 = mutex2.lock();
    }

    #[test]
    #[should_panic(expected = "poisoned LoroMutex")]
    fn test_poisoned_lock_panics_after_unwind() {
        let group = LoroLockGroup::new();
        let mutex = group.new_lock(42, LockKind::Txn);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock();
            panic!("poison the lock");
        }));

        let _ = mutex.lock();
    }
}

#[cfg(all(test, not(loom)))]
mod txn_gate_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn owner_thread_nests_and_writes_without_blocking() {
        let group = LoroLockGroup::new();
        let txn = group.new_lock(0, LockKind::Txn);
        let outer = txn.lock_exclusive();
        {
            let _inner = txn.lock_exclusive();
            *txn.lock() += 1;
        }
        // The nested guard must not release the gate.
        assert!(txn.txn_gate.active.load(Ordering::SeqCst));
        *txn.lock() += 1;
        drop(outer);
        assert!(!txn.txn_gate.active.load(Ordering::SeqCst));
        assert_eq!(*txn.lock(), 2);
    }

    #[test]
    fn other_writers_wait_for_the_compound_operation() {
        let group = LoroLockGroup::new();
        let txn = Arc::new(group.new_lock(Vec::new(), LockKind::Txn));
        let exclusive = txn.lock_exclusive();
        txn.lock().push("owner-1");
        let (done, finished) = mpsc::channel();
        let writer = {
            let txn = txn.clone();
            std::thread::spawn(move || {
                txn.lock().push("writer");
                done.send(()).unwrap();
            })
        };
        // The writer cannot run between the owner's two transaction sections.
        assert!(finished.recv_timeout(Duration::from_millis(100)).is_err());
        txn.lock().push("owner-2");
        drop(exclusive);
        finished.recv_timeout(Duration::from_secs(10)).unwrap();
        writer.join().unwrap();
        assert_eq!(*txn.lock(), vec!["owner-1", "owner-2", "writer"]);
    }

    #[test]
    fn a_panic_inside_the_compound_operation_does_not_block_later_writers() {
        let group = LoroLockGroup::new();
        let txn = Arc::new(group.new_lock(0, LockKind::Txn));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _exclusive = txn.lock_exclusive();
            panic!("undo failed");
        }));
        assert!(result.is_err());
        assert!(!txn.txn_gate.active.load(Ordering::SeqCst));
        let other = {
            let txn = txn.clone();
            std::thread::spawn(move || {
                *txn.lock() += 1;
                drop(txn.lock_exclusive());
            })
        };
        other.join().unwrap();
        drop(txn.lock_exclusive());
        assert_eq!(*txn.lock(), 1);
    }
}
