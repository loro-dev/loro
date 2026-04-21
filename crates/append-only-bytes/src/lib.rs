#![deny(clippy::undocumented_unsafe_blocks)]
#![doc = include_str!("../README.md")]

mod raw_bytes;
use std::{
    fmt::Debug,
    ops::{Deref, Index, RangeBounds},
    slice::SliceIndex,
    sync::Arc,
};

use raw_bytes::RawBytes;
#[cfg(feature = "serde")]
mod serde;

pub struct AppendOnlyBytes {
    raw: Arc<RawBytes>,
    len: usize,
}

impl Debug for AppendOnlyBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppendOnlyBytes")
            .field("data", &self.as_bytes())
            .field("len", &self.len)
            .finish()
    }
}

impl Clone for AppendOnlyBytes {
    fn clone(&self) -> Self {
        let new = RawBytes::with_capacity(self.capacity());
        // SAFETY: raw and new have at least self.len capacity
        unsafe {
            std::ptr::copy_nonoverlapping(self.raw.ptr(), new.ptr(), self.len);
        }

        Self {
            #[allow(clippy::arc_with_non_send_sync)]
            raw: Arc::new(new),
            len: self.len,
        }
    }
}

#[derive(Clone)]
pub struct BytesSlice {
    raw: Arc<RawBytes>,
    #[cfg(not(feature = "u32_range"))]
    start: usize,
    #[cfg(not(feature = "u32_range"))]
    end: usize,
    #[cfg(feature = "u32_range")]
    start: u32,
    #[cfg(feature = "u32_range")]
    end: u32,
}

impl Debug for BytesSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BytesSlice")
            .field("data", &&self[..])
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

impl PartialEq for BytesSlice {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for BytesSlice {}

impl PartialOrd for BytesSlice {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BytesSlice {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

// SAFETY: It's Send & Sync because it doesn't have interior mutability. And the owner of the type can only append data to it.
// All the existing data will never be changed.
unsafe impl Send for AppendOnlyBytes {}
// SAFETY: It's Send & Sync because it doesn't have interior mutability. And the owner of the type can only append data to it.
// All the existing data will never be changed.
unsafe impl Sync for AppendOnlyBytes {}

const MIN_CAPACITY: usize = 32;
impl AppendOnlyBytes {
    #[inline(always)]
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: data inside len is initialized
        unsafe { self.raw.slice(..self.len) }
    }

    #[inline(always)]
    pub fn with_capacity(capacity: usize) -> Self {
        #[allow(clippy::arc_with_non_send_sync)]
        let raw = Arc::new(RawBytes::with_capacity(capacity));
        Self { raw, len: 0 }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Truncate the visible length without clearing or moving the underlying bytes.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no live [`BytesSlice`] points to bytes in
    /// `len..self.len()`. Future appends may overwrite that range.
    #[inline(always)]
    pub unsafe fn truncate_unchecked(&mut self, len: usize) {
        assert!(len <= self.len);
        self.len = len;
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.raw.capacity()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    pub fn push_slice(&mut self, slice: &[u8]) {
        self.reserve(slice.len());
        // SAFETY: We have reserved enough space for the slice
        unsafe {
            std::ptr::copy_nonoverlapping(
                slice.as_ptr(),
                self.raw.ptr().add(self.len),
                slice.len(),
            );
            self.len += slice.len();
        }
    }

    #[inline(always)]
    pub fn push_str(&mut self, slice: &str) {
        self.push_slice(slice.as_bytes());
    }

    #[inline(always)]
    pub fn push(&mut self, byte: u8) {
        self.reserve(1);
        // SAFETY: We have reserved enough space for the byte
        unsafe {
            std::ptr::write(self.raw.ptr().add(self.len), byte);
            self.len += 1;
        }
    }

    #[inline]
    pub fn reserve(&mut self, size: usize) {
        let target_capacity = self.len() + size;
        if target_capacity > self.capacity() {
            let mut new_capacity = (self.capacity() * 2).max(MIN_CAPACITY);
            while new_capacity < target_capacity {
                new_capacity *= 2;
            }

            let src = std::mem::replace(self, Self::with_capacity(new_capacity));
            // SAFETY: copy from src to dst, both have at least the capacity of src.len()
            unsafe {
                std::ptr::copy_nonoverlapping(src.raw.ptr(), self.raw.ptr(), src.len());
                self.len = src.len();
            }
        }
    }

    #[inline]
    pub fn slice_str(&self, range: impl RangeBounds<usize>) -> Result<&str, std::str::Utf8Error> {
        let (start, end) = get_range(range, self.len());
        // SAFETY: data inside start..end is initialized
        std::str::from_utf8(unsafe { self.raw.slice(start..end) })
    }

    #[inline]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> BytesSlice {
        let (start, end) = get_range(range, self.len());
        BytesSlice::new(self.raw.clone(), start, end)
    }

    #[inline(always)]
    pub fn to_slice(self) -> BytesSlice {
        let end = self.len();
        BytesSlice::new(self.raw, 0, end)
    }
}

impl Default for AppendOnlyBytes {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn get_range(range: impl RangeBounds<usize>, max_len: usize) -> (usize, usize) {
    let start = match range.start_bound() {
        std::ops::Bound::Included(&v) => v,
        std::ops::Bound::Excluded(&v) => v + 1,
        std::ops::Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        std::ops::Bound::Included(&v) => v + 1,
        std::ops::Bound::Excluded(&v) => v,
        std::ops::Bound::Unbounded => max_len,
    };
    assert!(start <= end);
    assert!(end <= max_len);
    (start, end)
}

impl<I: SliceIndex<[u8]>> Index<I> for AppendOnlyBytes {
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        // SAFETY: data inside 0..self.len is initialized
        unsafe { Index::index(self.raw.slice(..self.len), index) }
    }
}

// SAFETY: It's Send & Sync because it doesn't have interior mutability. All the accessible data in this type will never be changed.
unsafe impl Send for BytesSlice {}
// SAFETY: It's Send & Sync because it doesn't have interior mutability. All the accessible data in this type will never be changed.
unsafe impl Sync for BytesSlice {}

#[cfg(not(feature = "u32_range"))]
type Int = usize;
#[cfg(feature = "u32_range")]
type Int = u32;

impl BytesSlice {
    #[inline(always)]
    fn new(raw: Arc<RawBytes>, start: usize, end: usize) -> Self {
        Self {
            raw,
            start: start as Int,
            end: end as Int,
        }
    }

    #[inline(always)]
    pub fn empty() -> Self {
        Self {
            #[allow(clippy::arc_with_non_send_sync)]
            raw: Arc::new(RawBytes::with_capacity(0)),
            start: 0,
            end: 0,
        }
    }

    #[inline(always)]
    fn bytes(&self) -> &[u8] {
        // SAFETY: data inside this range is guaranteed to be initialized
        unsafe { self.raw.slice(self.start()..self.end()) }
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: data inside this range is guaranteed to be initialized
        unsafe { self.raw.slice(self.start()..self.end()) }
    }

    #[inline(always)]
    #[allow(clippy::unnecessary_cast)]
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    #[allow(clippy::arc_with_non_send_sync)]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let new = RawBytes::with_capacity(bytes.len());
        // SAFETY: raw and new have at least self.len capacity
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), new.ptr(), bytes.len());
        }

        Self {
            raw: Arc::new(new),
            start: 0,
            end: bytes.len() as Int,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }

    #[inline(always)]
    #[allow(clippy::unnecessary_cast)]
    pub fn slice_clone(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
        let (start, end) = get_range(range, (self.end - self.start) as usize);
        Self::new(self.raw.clone(), self.start() + start, self.start() + end)
    }

    #[inline(always)]
    #[allow(clippy::unnecessary_cast)]
    pub fn slice_(&mut self, range: impl std::ops::RangeBounds<usize>) {
        let (start, end) = get_range(range, (self.end - self.start) as usize);
        self.end = self.start + end as Int;
        self.start += start as Int;
    }

    #[inline(always)]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.raw, &other.raw)
    }

    #[inline(always)]
    pub fn can_merge(&self, other: &Self) -> bool {
        self.ptr_eq(other) && self.end == other.start
    }

    #[inline(always)]
    pub fn try_merge(&mut self, other: &Self) -> Result<(), MergeFailed> {
        if self.can_merge(other) {
            self.end = other.end;
            Ok(())
        } else {
            Err(MergeFailed)
        }
    }

    #[inline]
    pub fn slice_str(&self, range: impl RangeBounds<usize>) -> Result<&str, std::str::Utf8Error> {
        let (start, end) = get_range(range, self.len());
        std::str::from_utf8(&self.deref()[start..end])
    }

    #[inline(always)]
    #[allow(clippy::unnecessary_cast)]
    pub fn start(&self) -> usize {
        self.start as usize
    }

    #[inline(always)]
    #[allow(clippy::unnecessary_cast)]
    pub fn end(&self) -> usize {
        self.end as usize
    }
}

#[derive(Debug)]
pub struct MergeFailed;

impl Deref for BytesSlice {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.bytes()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc::{self, Receiver, Sender},
        thread,
    };

    use super::*;
    #[test]
    fn test() {
        let mut a = AppendOnlyBytes::new();
        let mut count = 0;
        for _ in 0..100 {
            a.push(8);
            count += 1;
            assert_eq!(a.len(), count);
        }

        for _ in 0..100 {
            a.push_slice(&[1, 2]);
            count += 2;
            assert_eq!(a.len(), count);
        }
    }

    #[test]
    fn it_works() {
        let mut a = AppendOnlyBytes::new();
        a.push_str("123");
        assert_eq!(a.slice_str(0..1).unwrap(), "1");
        let b = a.slice(..);
        for _ in 0..10 {
            a.push_str("456");
            dbg!(a.slice_str(..).unwrap());
        }
        let c = a.slice(..);
        drop(a);
        dbg!(c.slice_str(..).unwrap());
        assert_eq!(c.len(), 33);
        assert_eq!(c.slice_str(..6).unwrap(), "123456");

        assert_eq!(b.deref(), "123".as_bytes());
    }

    #[test]
    fn push_large() {
        let mut a = AppendOnlyBytes::new();
        a.push_slice(&[1; 10000]);
        assert_eq!(a.as_bytes(), &[1; 10000]);
    }

    #[test]
    fn truncate_keeps_prefix_and_allows_reuse() {
        let mut a = AppendOnlyBytes::new();
        a.push_str("hello");
        let prefix = a.slice(..);
        a.push_str(" world");
        // SAFETY: `prefix` only points to bytes before the truncated suffix.
        unsafe { a.truncate_unchecked(5) };
        assert_eq!(a.as_bytes(), b"hello");
        a.push_str("!");
        assert_eq!(a.as_bytes(), b"hello!");
        assert_eq!(prefix.deref(), b"hello");
    }

    #[test]
    fn threads() {
        let mut a = AppendOnlyBytes::new();
        a.push_str("123");
        assert_eq!(a.slice_str(0..1).unwrap(), "1");
        let (tx, rx): (Sender<AppendOnlyBytes>, Receiver<AppendOnlyBytes>) = mpsc::channel();
        let b = a.slice(..);
        let t = thread::spawn(move || {
            for _ in 0..10 {
                a.push_str("456");
                dbg!(a.slice_str(..).unwrap());
            }
            let c = a.slice(..);
            tx.send(a).unwrap();
            dbg!(c.slice_str(..).unwrap());
            assert_eq!(c.len(), 33);
            assert_eq!(c.slice_str(..6).unwrap(), "123456");
        });
        let t1 = thread::spawn(move || {
            assert_eq!(b.deref(), "123".as_bytes());
            for _ in 0..10 {
                let c = b.slice_clone(0..1);
                assert_eq!(c.deref(), "1".as_bytes());
            }
        });

        let a = rx.recv().unwrap();
        assert_eq!(a.len(), 33);
        assert_eq!(&a[..6], "123456".as_bytes());
        t.join().unwrap();
        t1.join().unwrap()
    }

    #[test]
    fn from_bytes() {
        let a = BytesSlice::from_bytes(b"123");
        assert_eq!(a.len(), 3);
        assert_eq!(a.slice_str(..).unwrap(), "123");
    }
}
