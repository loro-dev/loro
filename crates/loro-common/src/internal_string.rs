use std::{fmt::Display, ops::Deref};

use serde::{Deserialize, Serialize};

#[repr(transparent)]
#[derive(Clone, Debug, Default, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct InternalString(string_cache::DefaultAtom);

impl<T: Into<string_cache::DefaultAtom>> From<T> for InternalString {
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
