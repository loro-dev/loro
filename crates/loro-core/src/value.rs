use fxhash::FxHashMap;

use crate::{container::ContainerID, InternalString, SmString};

/// [LoroValue] is used to represents the state of CRDT at a given version
#[derive(Debug, PartialEq, Clone)]
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
            InsertValue::Integer(i) => LoroValue::Integer(i),
            InsertValue::String(s) => LoroValue::String(s),
            InsertValue::Container(c) => LoroValue::Unresolved(c),
        }
    }
}

/// [InsertValue] can be inserted to Map or List
#[derive(Debug, PartialEq, Clone)]
pub enum InsertValue {
    Null,
    Bool(bool),
    Double(f64),
    Integer(i32),
    String(SmString),
    Container(ContainerID),
}
