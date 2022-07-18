use crate::value::LoroValue;

#[derive(Debug, PartialEq, Clone)]
pub struct Snapshot {
    value: LoroValue,
}

impl Snapshot {
    pub fn new(value: LoroValue) -> Self {
        Snapshot { value }
    }

    pub fn value(&self) -> &LoroValue {
        &self.value
    }
}
