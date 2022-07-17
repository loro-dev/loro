use crate::value::SnapshotValue;

#[derive(Debug, PartialEq, Clone)]
pub struct Snapshot {
    value: SnapshotValue,
}

impl Snapshot {
    pub fn new(value: SnapshotValue) -> Self {
        Snapshot { value }
    }

    pub fn value(&self) -> &SnapshotValue {
        &self.value
    }
}
