use std::{fmt::Display, ops::Deref};

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU32;

#[repr(transparent)]
#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct InternalString(string_cache::DefaultAtom);

#[cfg(debug_assertions)]
static mut INTERNAL_STRING_COUNT: AtomicU32 = AtomicU32::new(0);

#[cfg(debug_assertions)]
fn fetch_add() {
    unsafe {
        INTERNAL_STRING_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl<T: Into<string_cache::DefaultAtom>> From<T> for InternalString {
    #[inline(always)]
    fn from(value: T) -> Self {
        #[cfg(debug_assertions)]
        fetch_add();
        Self(value.into())
    }
}

impl From<&InternalString> for String {
    #[inline(always)]
    fn from(value: &InternalString) -> Self {
        value.0.to_string()
    }
}

impl From<&InternalString> for InternalString {
    #[inline(always)]
    fn from(value: &InternalString) -> Self {
        #[cfg(debug_assertions)]
        fetch_add();
        value.clone()
    }
}

impl Clone for InternalString {
    fn clone(&self) -> Self {
        #[cfg(debug_assertions)]
        fetch_add();
        Self(self.0.clone())
    }
}

impl Default for InternalString {
    fn default() -> Self {
        #[cfg(debug_assertions)]
        fetch_add();
        Self(Default::default())
    }
}

impl Display for InternalString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for InternalString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[cfg(debug_assertions)]
impl Drop for InternalString {
    fn drop(&mut self) {
        unsafe {
            INTERNAL_STRING_COUNT.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Returns the total number of internal strings created.
///
/// It should not exceed 100K for the optimal performance.
/// See https://github.com/zxch3n/test-string-cache
#[cfg(debug_assertions)]
pub fn get_total_internal_string_size() -> usize {
    unsafe { INTERNAL_STRING_COUNT.load(std::sync::atomic::Ordering::Relaxed) as usize }
}
