use loro_common::LoroValue;

pub trait EstimatedSize {
    /// Estimate the storage size of the object in bytes
    fn estimate_storage_size(&self) -> usize;
}

impl EstimatedSize for LoroValue {
    fn estimate_storage_size(&self) -> usize {
        match self {
            Self::Null => 1,
            Self::Bool(_) => 1,
            Self::Double(_) => 8,
            Self::I64(i) => 1 + (64 - i.leading_zeros()) as usize / 7,
            Self::Binary(b) => b.len() + 1,
            Self::String(s) => s.len() + 1,
            Self::List(l) => 3 + l.iter().map(|x| x.estimate_storage_size()).sum::<usize>(),
            Self::Map(m) => {
                3 + m
                    .iter()
                    .map(|(k, v)| k.len() + 3 + v.estimate_storage_size())
                    .sum::<usize>()
            }
            Self::Container(_) => 6,
        }
    }
}
