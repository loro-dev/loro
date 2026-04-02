#[cfg(loom)]
pub use loom::thread;
#[cfg(not(loom))]
pub use std::thread;

#[cfg(loom)]
pub use loom::sync::{
    LockResult, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
#[cfg(not(loom))]
pub use std::sync::{
    LockResult, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

#[cfg(loom)]
pub use loom::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize};
#[cfg(not(loom))]
pub use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize};

#[cfg(loom)]
pub(crate) use my_thread_local::ThreadLocal;
#[cfg(not(loom))]
pub(crate) use thread_local::ThreadLocal;

fn unpoison<T>(result: LockResult<T>) -> T {
    result.unwrap_or_else(PoisonError::into_inner)
}

pub(crate) trait MutexExt<T: ?Sized> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> MutexExt<T> for Mutex<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T> {
        unpoison(self.lock())
    }
}

#[allow(dead_code)]
pub(crate) trait RwLockExt<T> {
    fn read_unpoisoned(&self) -> RwLockReadGuard<'_, T>;
    fn write_unpoisoned(&self) -> RwLockWriteGuard<'_, T>;
    fn into_inner_unpoisoned(self) -> T
    where
        Self: Sized;
}

impl<T> RwLockExt<T> for RwLock<T> {
    fn read_unpoisoned(&self) -> RwLockReadGuard<'_, T> {
        unpoison(self.read())
    }

    fn write_unpoisoned(&self) -> RwLockWriteGuard<'_, T> {
        unpoison(self.write())
    }

    fn into_inner_unpoisoned(self) -> T
    where
        Self: Sized,
    {
        self.into_inner().unwrap_or_else(PoisonError::into_inner)
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
            let mut content = self.content.lock_unpoisoned();
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
    fn mutex_lock_recovers_after_poison() {
        let lock = Mutex::new(7);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.lock_unpoisoned();
            panic!("poison mutex");
        }));

        assert_eq!(*lock.lock_unpoisoned(), 7);
    }

    #[test]
    fn rwlock_recovers_after_poison() {
        let lock = RwLock::new(7);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = lock.write_unpoisoned();
            *guard = 9;
            panic!("poison rwlock");
        }));

        assert_eq!(*lock.read_unpoisoned(), 9);
        *lock.write_unpoisoned() = 11;
        assert_eq!(lock.into_inner_unpoisoned(), 11);
    }
}
