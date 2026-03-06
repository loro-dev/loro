use loro_common::LoroValue;

pub trait EstimatedSize {
    /// Estimate the storage size of the object in bytes
    fn estimate_storage_size(&self) -> usize;
}

impl EstimatedSize for LoroValue {
    fn estimate_storage_size(&self) -> usize {
        match self {
            LoroValue::Null => 1,
            LoroValue::Bool(_) => 1,
            LoroValue::Double(_) => 8,
            LoroValue::I64(i) => 1 + (64 - i.leading_zeros()) as usize / 7,
            LoroValue::Binary(b) => b.len() + 1,
            LoroValue::String(s) => s.len() + 1,
            LoroValue::List(l) => 3 + l.iter().map(|x| x.estimate_storage_size()).sum::<usize>(),
            LoroValue::Map(m) => {
                3 + m
                    .iter()
                    .map(|(k, v)| k.len() + 3 + v.estimate_storage_size())
                    .sum::<usize>()
            }
            LoroValue::Container(_) => 6,
        }
    }
}
