pub enum BasicValueKind {
    Null = 0,
    True = 1,
    False = 2,
    I64 = 3,
    F64 = 4,
    Str = 5,
    DeltaInt = 6,
    Array = 7,
    Map,
    Binary,
    ContainerType,
    DeleteOnce,
    DeleteSeq,
    MarkStart,
    TreeMove,

    Unknown = 65536,
}

pub enum ExtraValueKind {
    Unknown,
}

pub trait ValueEncode {
    fn bytes_len(&self) -> usize;
    fn decode(bytes: &[u8]) -> Self;
    fn encode(&self, bytes: &mut Vec<u8>);
}
