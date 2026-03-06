#[cfg(loom)]
pub use loom::thread;
#[cfg(not(loom))]
pub use std::thread;

#[cfg(loom)]
pub use loom::sync::{Mutex, MutexGuard, RwLock};
#[cfg(not(loom))]
pub use std::sync::{Mutex, MutexGuard, RwLock};

#[cfg(loom)]
pub use loom::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize};
#[cfg(not(loom))]
pub use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize};

#[cfg(loom)]
pub(crate) use my_thread_local::ThreadLocal;
#[cfg(not(loom))]
pub(crate) use thread_local::ThreadLocal;

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
            let mut content = self.content.lock().unwrap();
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
