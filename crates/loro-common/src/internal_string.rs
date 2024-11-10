use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Deref, sync::Arc};

#[repr(transparent)]
#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct InternalString(InternalStringInner);

#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
