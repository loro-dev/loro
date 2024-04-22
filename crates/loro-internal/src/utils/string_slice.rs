use std::{fmt::Debug, ops::Deref};

use append_only_bytes::BytesSlice;
use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};
use rle::Mergable;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    container::richtext::richtext_state::{unicode_to_utf8_index, utf16_to_utf8_index},
    delta::DeltaValue,
};

use super::utf16::{count_unicode_chars, count_utf16_len};

#[derive(Clone)]
pub struct StringSlice {
    bytes: Variant,
}

impl PartialEq for StringSlice {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for StringSlice {}

#[derive(Clone, PartialEq, Eq)]
enum Variant {
    BytesSlice(BytesSlice),
    Owned(String),
}

impl From<String> for StringSlice {
    fn from(s: String) -> Self {
        Self {
            bytes: Variant::Owned(s),
        }
    }
}

impl From<BytesSlice> for StringSlice {
    fn from(s: BytesSlice) -> Self {
        Self::new(s)
    }
}

impl From<&str> for StringSlice {
    fn from(s: &str) -> Self {
        Self {
            bytes: Variant::Owned(s.to_string()),
        }
    }
}

impl Debug for StringSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StringSlice")
            .field("bytes", &self.as_str())
            .finish()
    }
}

impl StringSlice {
    pub fn new(s: BytesSlice) -> Self {
        std::str::from_utf8(&s).unwrap();
        Self {
            bytes: Variant::BytesSlice(s),
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.bytes {
            // SAFETY: `bytes` is always valid utf8
            Variant::BytesSlice(s) => unsafe { std::str::from_utf8_unchecked(s) },
            Variant::Owned(s) => s,
        }
    }

    pub fn len_bytes(&self) -> usize {
        match &self.bytes {
            Variant::BytesSlice(s) => s.len(),
            Variant::Owned(s) => s.len(),
        }
    }

    fn bytes(&self) -> &[u8] {
        match &self.bytes {
            Variant::BytesSlice(s) => s.deref(),
            Variant::Owned(s) => s.as_bytes(),
        }
    }

    pub fn len_unicode(&self) -> usize {
        count_unicode_chars(self.bytes())
    }

    pub fn len_utf16(&self) -> usize {
        count_utf16_len(self.bytes())
    }

    pub fn is_empty(&self) -> bool {
        self.bytes().is_empty()
    }

    pub fn extend(&mut self, s: &str) {
        match &mut self.bytes {
            Variant::BytesSlice(_) => {
                *self = Self {
                    bytes: Variant::Owned(format!("{}{}", self.as_str(), s)),
                }
            }
            Variant::Owned(v) => {
                v.push_str(s);
            }
        }
    }
}

impl std::fmt::Display for StringSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StringSlice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(s.into())
    }
}

impl Serialize for StringSlice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl DeltaValue for StringSlice {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        match (&mut self.bytes, &other.bytes) {
            (Variant::BytesSlice(s), Variant::BytesSlice(o)) => match s.try_merge(o) {
                Ok(_) => Ok(()),
                Err(_) => Err(other),
            },
            (Variant::Owned(s), _) => {
                s.push_str(other.as_str());
                Ok(())
            }
            _ => Err(other),
        }
    }

    fn take(&mut self, length: usize) -> Self {
        let length = if cfg!(feature = "wasm") {
            utf16_to_utf8_index(self.as_str(), length).unwrap()
        } else {
            unicode_to_utf8_index(self.as_str(), length).unwrap()
        };

        match &mut self.bytes {
            Variant::BytesSlice(s) => {
                let mut other = s.slice_clone(length..);
                s.slice_(..length);
                std::mem::swap(s, &mut other);
                Self {
                    bytes: Variant::BytesSlice(other),
                }
            }
            Variant::Owned(s) => {
                let mut other = s.split_off(length);
                std::mem::swap(s, &mut other);
                Self {
                    bytes: Variant::Owned(other),
                }
            }
        }
    }

    /// Unicode length of the string
    /// Utf16 length when in WASM
    fn length(&self) -> usize {
        if cfg!(feature = "wasm") {
            count_utf16_len(self.bytes())
        } else {
            count_unicode_chars(self.bytes())
        }
    }
}

impl HasLength for StringSlice {
    fn rle_len(&self) -> usize {
        if cfg!(feature = "wasm") {
            count_utf16_len(self.bytes())
        } else {
            count_unicode_chars(self.bytes())
        }
    }
}

impl TryInsert for StringSlice {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        // let pos = if cfg!(feature = "wasm") {
        //     utf16_to_utf8_index(self.as_str(), pos).unwrap()
        // } else {
        //     unicode_to_utf8_index(self.as_str(), pos).unwrap()
        // };

        // match (&mut self.bytes, &elem.bytes) {
        //     (Variant::Owned(a), Variant::Owned(b))
        //         // TODO: Extract magic num
        //         if a.capacity() >= a.len() + b.len() && a.capacity() < 128 =>
        //     {
        //         a.insert_str(pos, b.as_str());
        //         Ok(())
        //     }
        //     _ => Err(elem),
        // }
        Err(elem)
    }
}

impl Mergeable for StringSlice {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (&self.bytes, &rhs.bytes) {
            (Variant::BytesSlice(a), Variant::BytesSlice(b)) => a.can_merge(&b),
            (Variant::Owned(a), Variant::Owned(b)) => a.len() + b.len() <= a.capacity(),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (&mut self.bytes, &rhs.bytes) {
            (Variant::BytesSlice(a), Variant::BytesSlice(b)) => a.merge(&b, &()),
            (Variant::Owned(a), Variant::Owned(b)) => a.push_str(b.as_str()),
            _ => {}
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (&mut self.bytes, &left.bytes) {
            (Variant::BytesSlice(a), Variant::BytesSlice(b)) => {
                let mut new = b.clone();
                new.merge(a, &());
                *a = new;
            }
            (Variant::Owned(a), Variant::Owned(b)) => {
                a.insert_str(0, b.as_str());
            }
            _ => {}
        }
    }
}

impl Sliceable for StringSlice {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        let range = if cfg!(feature = "wasm") {
            let start = utf16_to_utf8_index(self.as_str(), range.start).unwrap();
            let end = utf16_to_utf8_index(self.as_str(), range.end).unwrap();
            start..end
        } else {
            let start = unicode_to_utf8_index(self.as_str(), range.start).unwrap();
            let end = unicode_to_utf8_index(self.as_str(), range.end).unwrap();
            start..end
        };

        let bytes = match &self.bytes {
            Variant::BytesSlice(s) => Variant::BytesSlice(s.slice_clone(range)),
            Variant::Owned(s) => Variant::Owned(s[range].to_string()),
        };

        Self { bytes }
    }

    fn split(&mut self, pos: usize) -> Self {
        let pos = if cfg!(feature = "wasm") {
            utf16_to_utf8_index(self.as_str(), pos).unwrap()
        } else {
            unicode_to_utf8_index(self.as_str(), pos).unwrap()
        };

        let bytes = match &mut self.bytes {
            Variant::BytesSlice(s) => {
                let other = s.slice_clone(pos..);
                s.slice_(..pos);
                Variant::BytesSlice(other)
            }
            Variant::Owned(s) => {
                let other = s.split_off(pos);
                Variant::Owned(other)
            }
        };

        Self { bytes }
    }
}

impl loro_delta::delta_trait::DeltaValue for StringSlice {}

pub fn unicode_range_to_byte_range(s: &str, start: usize, end: usize) -> (usize, usize) {
    debug_assert!(start <= end);
    let start_unicode_index = start;
    let end_unicode_index = end;
    let mut current_utf8_index = 0;
    let mut start_byte = 0;
    let mut end_byte = s.len();
    for (current_unicode_index, c) in s.chars().enumerate() {
        if current_unicode_index == start_unicode_index {
            start_byte = current_utf8_index;
        }

        if current_unicode_index == end_unicode_index {
            end_byte = current_utf8_index;
            break;
        }

        current_utf8_index += c.len_utf8();
    }

    (start_byte, end_byte)
}
