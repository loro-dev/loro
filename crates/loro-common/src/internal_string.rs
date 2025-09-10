use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::slice;
use std::sync::LazyLock;
use std::{
    fmt::Display,
    num::NonZeroU64,
    ops::Deref,
    sync::{atomic::AtomicUsize, Arc, Mutex},
};

const DYNAMIC_TAG: u8 = 0b_00;
const INLINE_TAG: u8 = 0b_01;
const TAG_MASK: u64 = 0b_11;
const LEN_OFFSET: u64 = 4;
const LEN_MASK: u64 = 0xF0;

#[repr(transparent)]
#[derive(Clone)]
pub struct InternalString {
    unsafe_data: UnsafeData,
}

union UnsafeData {
    inline: NonZeroU64,
    dynamic: *const Box<str>,
}

unsafe impl Sync for UnsafeData {}
unsafe impl Send for UnsafeData {}

impl UnsafeData {
    #[inline(always)]
    fn is_inline(&self) -> bool {
        unsafe { (self.inline.get() & TAG_MASK) as u8 == INLINE_TAG }
    }
}

impl Clone for UnsafeData {
    fn clone(&self) -> Self {
        if self.is_inline() {
            Self {
                inline: unsafe { self.inline },
            }
        } else {
            unsafe {
                Arc::increment_strong_count(self.dynamic);
                Self {
                    dynamic: self.dynamic,
                }
            }
        }
    }
}

impl std::fmt::Debug for InternalString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InternalString(")?;
        std::fmt::Debug::fmt(self.as_str(), f)?;
        f.write_str(")")
    }
}

impl std::hash::Hash for InternalString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq for InternalString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for InternalString {}

impl PartialOrd for InternalString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InternalString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Serialize for InternalString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InternalString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(InternalString::from(s.as_str()))
    }
}

impl Default for InternalString {
    fn default() -> Self {
        let v: u64 = INLINE_TAG as u64;
        Self {
            // SAFETY: INLINE_TAG is non-zero
            unsafe_data: UnsafeData {
                inline: unsafe { NonZeroU64::new_unchecked(v) },
            },
        }
    }
}

impl InternalString {
    pub fn as_str(&self) -> &str {
        unsafe {
            match (self.unsafe_data.inline.get() & TAG_MASK) as u8 {
                INLINE_TAG => {
                    let len = (self.unsafe_data.inline.get() & LEN_MASK) >> LEN_OFFSET;
                    let src = inline_atom_slice(&self.unsafe_data.inline);
                    // SAFETY: the chosen range is guaranteed to be valid str
                    std::str::from_utf8_unchecked(&src[..(len as usize)])
                }
                DYNAMIC_TAG => {
                    let ptr = self.unsafe_data.dynamic;
                    // SAFETY: ptr is valid
                    (*ptr).deref()
                }
                _ => unreachable!(),
            }
        }
    }
}

impl AsRef<str> for InternalString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for InternalString {
    #[inline(always)]
    fn from(s: &str) -> Self {
        if s.len() <= 7 {
            let mut v: u64 = (INLINE_TAG as u64) | ((s.len() as u64) << LEN_OFFSET);
            let arr = inline_atom_slice_mut(&mut v);
            arr[..s.len()].copy_from_slice(s.as_bytes());
            Self {
                unsafe_data: UnsafeData {
                    // SAFETY: The tag is 1
                    inline: unsafe { NonZeroU64::new_unchecked(v) },
                },
            }
        } else {
            let ans: Arc<Box<str>> = get_or_init_internalized_string(s);
            let raw = Arc::into_raw(ans);
            // SAFETY: Pointer is non-zero
            Self {
                unsafe_data: UnsafeData { dynamic: raw },
            }
        }
    }
}

#[inline(always)]
fn inline_atom_slice(x: &NonZeroU64) -> &[u8] {
    unsafe {
        let x: *const NonZeroU64 = x;
        let mut data = x as *const u8;
        // All except the lowest byte, which is first in little-endian, last in big-endian.
        if cfg!(target_endian = "little") {
            data = data.offset(1);
        }
        let len = 7;
        slice::from_raw_parts(data, len)
    }
}

#[inline(always)]
fn inline_atom_slice_mut(x: &mut u64) -> &mut [u8] {
    unsafe {
        let x: *mut u64 = x;
        let mut data = x as *mut u8;
        // All except the lowest byte, which is first in little-endian, last in big-endian.
        if cfg!(target_endian = "little") {
            data = data.offset(1);
        }
        let len = 7;
        slice::from_raw_parts_mut(data, len)
    }
}

impl From<String> for InternalString {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl From<&InternalString> for String {
    #[inline(always)]
    fn from(value: &InternalString) -> Self {
        value.as_str().to_string()
    }
}

impl Display for InternalString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl Deref for InternalString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[derive(Hash, PartialEq, Eq)]
struct ArcWrapper(Arc<Box<str>>);

impl Borrow<str> for ArcWrapper {
    fn borrow(&self) -> &str {
        &self.0
    }
}

static STRING_SET: LazyLock<Mutex<FxHashSet<ArcWrapper>>> =
    LazyLock::new(|| Mutex::new(FxHashSet::default()));

fn get_or_init_internalized_string(s: &str) -> Arc<Box<str>> {
    static MAX_MET_CACHE_SIZE: AtomicUsize = AtomicUsize::new(1 << 16);

    let mut set = STRING_SET.lock().unwrap();
    if let Some(v) = set.get(s) {
        v.0.clone()
    } else {
        let ans: Arc<Box<str>> = Arc::new(Box::from(s));
        set.insert(ArcWrapper(ans.clone()));
        let max = MAX_MET_CACHE_SIZE.load(std::sync::atomic::Ordering::Relaxed);
        if set.capacity() >= max {
            let old = set.len();
            set.retain(|s| Arc::strong_count(&s.0) > 1);
            let new = set.len();
            if old - new > new / 2 {
                set.shrink_to_fit();
            }

            MAX_MET_CACHE_SIZE.store(max * 2, std::sync::atomic::Ordering::Relaxed);
        }

        ans
    }
}

fn drop_cache(s: Arc<Box<str>>) {
    let mut set = STRING_SET.lock().unwrap();
    set.remove(&ArcWrapper(s));
    if set.len() < set.capacity() / 2 && set.capacity() > 128 {
        set.shrink_to_fit();
    }
}

impl Drop for InternalString {
    fn drop(&mut self) {
        unsafe {
            if (self.unsafe_data.inline.get() & TAG_MASK) as u8 == DYNAMIC_TAG {
                let ptr = self.unsafe_data.dynamic;
                // SAFETY: ptr is a valid Arc
                let arc: Arc<Box<str>> = Arc::from_raw(ptr);
                if Arc::strong_count(&arc) == 2 {
                    drop_cache(arc);
                } else {
                    drop(arc)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_cache() {
        let s1 = InternalString::from("hello");
        let s3 = InternalString::from("world");

        // Content should match
        assert_eq!("hello", s1.as_str());
        assert_eq!(s3.as_str(), "world");
    }

    #[test]
    fn test_long_string_cache() {
        let long_str1 = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
        let long_str2 = "A very long string that contains lots of repeated characters: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

        let s1 = InternalString::from(long_str1);
        let s2 = InternalString::from(long_str1);
        let s3 = InternalString::from(long_str2);

        // Same long strings should be equal
        assert_eq!(s1, s2);

        // Different long strings should be different
        assert_ne!(s1, s3);

        // Content should match exactly
        assert_eq!(s1.as_str(), long_str1);
        assert_eq!(s1.as_str(), long_str1);
        assert_eq!(s2.as_str(), long_str1);
        assert_eq!(s3.as_str(), long_str2);

        // Internal pointers should be same for equal strings
        assert!(std::ptr::eq(s1.as_str().as_ptr(), s2.as_str().as_ptr()));
        assert!(!std::ptr::eq(s1.as_str().as_ptr(), s3.as_str().as_ptr()));
    }

    #[test]
    fn test_long_string_cache_drop() {
        {
            let set = STRING_SET.lock().unwrap();
            assert_eq!(set.len(), 0);
        }
        {
            let s1 = InternalString::from("hello".repeat(10));
            let s2 = InternalString::from("hello".repeat(10));
            assert!(std::ptr::eq(s1.as_str().as_ptr(), s2.as_str().as_ptr()));
        }
        let set = STRING_SET.lock().unwrap();
        assert_eq!(set.len(), 0);
    }
}
