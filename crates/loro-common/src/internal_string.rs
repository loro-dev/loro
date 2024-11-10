use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Deref, sync::Arc};

#[repr(transparent)]
#[derive(Clone)]
pub struct InternalString(InternalStringInner);

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

#[derive(Clone)]
enum InternalStringInner {
    Small { len: u8, data: [u8; 7] },
    Large(Arc<str>),
}

impl Default for InternalString {
    fn default() -> Self {
        Self(InternalStringInner::Small {
            len: 0,
            data: [0u8; 7],
        })
    }
}

impl InternalString {
    pub fn as_str(&self) -> &str {
        match &self.0 {
            InternalStringInner::Small { len, data } => unsafe {
                std::str::from_utf8_unchecked(&data[..*len as usize])
            },
            InternalStringInner::Large(s) => s,
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
            let mut arr = [0u8; 7];
            arr[..s.len()].copy_from_slice(s.as_bytes());
            Self(InternalStringInner::Small {
                len: s.len() as u8,
                data: arr,
            })
        } else {
            Self(InternalStringInner::Large(Arc::from(s)))
        }
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
