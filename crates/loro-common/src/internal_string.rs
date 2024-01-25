use std::{fmt::Display, ops::Deref, sync::Arc};

use serde::{Deserialize, Serialize};

#[repr(transparent)]
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct InternalString(Arc<str>);

impl Default for InternalString {
    #[inline(always)]
    fn default() -> Self {
        let s = String::new();
        Self(s.into())
    }
}

impl Serialize for InternalString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'a> Deserialize<'a> for InternalString {
    fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self(s.into()))
    }
}

impl<T: Into<Arc<str>>> From<T> for InternalString {
    #[inline(always)]
    fn from(value: T) -> Self {
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
        value.clone()
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
