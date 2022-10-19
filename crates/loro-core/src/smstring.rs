use std::ops::DerefMut;

use std::ops::Deref;

use serde::Serialize;
use smartstring::LazyCompact;

use smartstring::SmartString;

#[repr(transparent)]
#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone, Default)]
pub struct SmString(pub(crate) SmartString<LazyCompact>);

impl Deref for SmString {
    type Target = SmartString<LazyCompact>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SmString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl SmString {
    pub fn new() -> Self {
        SmString(SmartString::new())
    }
}

impl From<SmartString<LazyCompact>> for SmString {
    fn from(s: SmartString<LazyCompact>) -> Self {
        SmString(s)
    }
}

impl From<String> for SmString {
    fn from(s: String) -> Self {
        SmString(s.into())
    }
}

impl Serialize for SmString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}
