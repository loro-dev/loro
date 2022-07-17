use fxhash::FxHashMap;

use crate::{container::ContainerID, InternalString, SmString};

/// [SnapshotValue] is used to represents the state at a given time
#[derive(Debug, PartialEq, Clone)]
pub enum SnapshotValue {
    Null,
    Bool(bool),
    Double(f64),
    Integer(i32),
    String(SmString),
    List(Vec<SnapshotValue>),
    Map(FxHashMap<InternalString, SnapshotValue>),
    Unresolved(ContainerID),
}

impl Default for SnapshotValue {
    fn default() -> Self {
        SnapshotValue::Null
    }
}

impl From<InsertValue> for SnapshotValue {
    fn from(v: InsertValue) -> Self {
        match v {
            InsertValue::Null => SnapshotValue::Null,
            InsertValue::Bool(b) => SnapshotValue::Bool(b),
            InsertValue::Double(d) => SnapshotValue::Double(d),
            InsertValue::Integer(i) => SnapshotValue::Integer(i),
            InsertValue::String(s) => SnapshotValue::String(s),
            InsertValue::Container(c) => SnapshotValue::Unresolved(c),
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
