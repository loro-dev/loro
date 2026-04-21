#[cfg(loom)]
pub use loom::thread;
#[cfg(not(loom))]
pub use std::thread;

#[cfg(loom)]
mod raw {
    pub use loom::sync::{
        LockResult, Mutex as RawMutex, MutexGuard, RwLock as RawRwLock, RwLockReadGuard,
        RwLockWriteGuard,
    };
}
#[cfg(not(loom))]
mod raw {
    pub use std::sync::{
        LockResult, Mutex as RawMutex, MutexGuard, RwLock as RawRwLock, RwLockReadGuard,
        RwLockWriteGuard,
    };
}

pub use raw::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

#[cfg(loom)]
pub use loom::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize};
#[cfg(not(loom))]
pub use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize};

#[cfg(loom)]
pub(crate) use my_thread_local::ThreadLocal;
#[cfg(not(loom))]
pub(crate) use thread_local::ThreadLocal;

fn expect_not_poisoned<T>(result: raw::LockResult<T>, lock_kind: &str) -> T {
    result.unwrap_or_else(|_| panic!("poisoned {lock_kind}"))
}

#[derive(Debug)]
pub struct Mutex<T: ?Sized> {
    inner: raw::RawMutex<T>,
}

impl<T> Mutex<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: raw::RawMutex::new(value),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.lock_with_kind("mutex")
    }

    pub(crate) fn lock_with_kind(&self, lock_kind: &str) -> MutexGuard<'_, T> {
        expect_not_poisoned(self.inner.lock(), lock_kind)
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.inner.try_lock().is_err()
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[derive(Debug)]
pub struct RwLock<T> {
    inner: raw::RawRwLock<T>,
}

impl<T> RwLock<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: raw::RawRwLock::new(value),
        }
    }

    pub fn into_inner(self) -> T {
        expect_not_poisoned(self.inner.into_inner(), "rwlock")
    }
}

impl<T> RwLock<T> {
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        expect_not_poisoned(self.inner.read(), "rwlock")
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        expect_not_poisoned(self.inner.write(), "rwlock")
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[cfg(loom)]
mod my_thread_local {
    use std::sync::Arc;

    use super::thread;
    use super::Mutex;
    use rustc_hash::FxHashMap;

    #[derive(Debug)]
    pub(crate) struct ThreadLocal<T> {
        content: Arc<Mutex<FxHashMap<thread::ThreadId, Arc<T>>>>,
    }

    impl<T: Default> ThreadLocal<T> {
        pub fn new() -> Self {
            Self {
                content: Arc::new(Mutex::new(FxHashMap::default())),
            }
        }

        pub fn get_or_default(&self) -> Arc<T> {
            let mut content = self.content.lock();
            let v = content
                .entry(thread::current().id())
                .or_insert_with(|| Arc::new(T::default()));
            v.clone()
        }
    }

    impl<T> Clone for ThreadLocal<T> {
        fn clone(&self) -> Self {
            Self {
                content: self.content.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "poisoned mutex")]
    fn mutex_lock_panics_after_poison() {
        let lock = Mutex::new(7);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.lock();
            panic!("poison mutex");
        }));

        drop(lock.lock());
    }

    #[test]
    #[should_panic(expected = "poisoned rwlock")]
    fn rwlock_read_panics_after_poison() {
        let lock = RwLock::new(7);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = lock.write();
            *guard = 9;
            panic!("poison rwlock");
        }));

        drop(lock.read());
    }
}
