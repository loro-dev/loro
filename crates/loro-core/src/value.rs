use fxhash::FxHashMap;

use crate::{container::ContainerID, smstring::SmString, InternalString};

/// [LoroValue] is used to represents the state of CRDT at a given version
#[derive(Debug, PartialEq, Clone, serde::Serialize)]
pub enum LoroValue {
    Null,
    Bool(bool),
    Double(f64),
    Integer(i32),
    String(SmString),
    List(Vec<LoroValue>),
    Map(FxHashMap<InternalString, LoroValue>),
    Unresolved(ContainerID),
}

impl Default for LoroValue {
    fn default() -> Self {
        LoroValue::Null
    }
}

impl From<InsertValue> for LoroValue {
    fn from(v: InsertValue) -> Self {
        match v {
            InsertValue::Null => LoroValue::Null,
            InsertValue::Bool(b) => LoroValue::Bool(b),
            InsertValue::Double(d) => LoroValue::Double(d),
            InsertValue::Int32(i) => LoroValue::Integer(i),
            InsertValue::String(s) => LoroValue::String(s),
            InsertValue::Container(c) => LoroValue::Unresolved(c),
        }
    }
}

impl From<LoroValue> for InsertValue {
    fn from(v: LoroValue) -> Self {
        match v {
            LoroValue::Null => InsertValue::Null,
            LoroValue::Bool(b) => InsertValue::Bool(b),
            LoroValue::Double(d) => InsertValue::Double(d),
            LoroValue::Integer(i) => InsertValue::Int32(i),
            LoroValue::String(s) => InsertValue::String(s),
            LoroValue::Unresolved(c) => InsertValue::Container(c),
            _ => unreachable!("Unsupported convert from LoroValue to InsertValue"),
        }
    }
}

// stupid getter and is_xxx
impl LoroValue {
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, LoroValue::Null)
    }

    #[inline]
    pub fn is_bool(&self) -> bool {
        matches!(self, LoroValue::Bool(_))
    }

    #[inline]
    pub fn is_double(&self) -> bool {
        matches!(self, LoroValue::Double(_))
    }

    #[inline]
    pub fn is_integer(&self) -> bool {
        matches!(self, LoroValue::Integer(_))
    }

    #[inline]
    pub fn is_string(&self) -> bool {
        matches!(self, LoroValue::String(_))
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self, LoroValue::List(_))
    }

    #[inline]
    pub fn is_map(&self) -> bool {
        matches!(self, LoroValue::Map(_))
    }

    #[inline]
    pub fn is_unresolved(&self) -> bool {
        matches!(self, LoroValue::Unresolved(_))
    }

    #[inline]
    pub fn is_resolved(&self) -> bool {
        !self.is_unresolved()
    }

    #[inline]
    pub fn to_map(&self) -> Option<&FxHashMap<InternalString, LoroValue>> {
        match self {
            LoroValue::Map(m) => Some(m),
            _ => None,
        }
    }

    #[inline]
    pub fn to_list(&self) -> Option<&Vec<LoroValue>> {
        match self {
            LoroValue::List(l) => Some(l),
            _ => None,
        }
    }

    #[inline]
    pub fn to_string(&self) -> Option<&SmString> {
        match self {
            LoroValue::String(s) => Some(s),
            _ => None,
        }
    }

    #[inline]
    pub fn to_integer(&self) -> Option<i32> {
        match self {
            LoroValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    #[inline]
    pub fn to_double(&self) -> Option<f64> {
        match self {
            LoroValue::Double(d) => Some(*d),
            _ => None,
        }
    }

    #[inline]
    pub fn to_bool(&self) -> Option<bool> {
        match self {
            LoroValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    #[inline]
    pub fn to_container(&self) -> Option<&ContainerID> {
        match self {
            LoroValue::Unresolved(c) => Some(c),
            _ => None,
        }
    }
}

/// [InsertValue] can be inserted to Map or List
#[derive(Debug, PartialEq, Clone)]
pub enum InsertValue {
    Null,
    Bool(bool),
    Double(f64),
    Int32(i32),
    String(SmString),
    Container(ContainerID),
}

#[cfg(test)]
pub(crate) mod proptest {
    use proptest::prelude::*;
    use proptest::{arbitrary::Arbitrary, prop_oneof};

    use crate::container::ContainerID;
    use crate::LoroValue;

    use super::InsertValue;

    pub fn gen_insert_value() -> impl Strategy<Value = InsertValue> {
        prop_oneof![
            Just(InsertValue::Null),
            any::<f64>().prop_map(InsertValue::Double),
            any::<i32>().prop_map(InsertValue::Int32),
            any::<bool>().prop_map(InsertValue::Bool),
            any::<String>().prop_map(|s| InsertValue::String(s.into())),
        ]
    }
}
