///|
const MAX_COLLECTION_SIZE : UInt64 = 268435456UL

///|
pub enum LoroValue {
  Null
  True
  False
  I64(Int64)
  F64(Double)
  Str(String)
  Binary(Bytes)
  List(Array[LoroValue])
  Map(Array[(UInt64, LoroValue)]) // (key_idx, value)
  ContainerType(Byte)
} derive(Eq, Show)

// NOTE: MoonBit makes enum constructors read-only across packages. Expose explicit
// constructor functions for callers (e.g. `loro_doc`).

///|
pub fn LoroValue::null() -> LoroValue {
  LoroValue::Null
}

///|
pub fn LoroValue::true_() -> LoroValue {
  LoroValue::True
}

///|
pub fn LoroValue::false_() -> LoroValue {
  LoroValue::False
}

///|
pub fn LoroValue::i64(x : Int64) -> LoroValue {
  LoroValue::I64(x)
}

///|
pub fn LoroValue::f64(x : Double) -> LoroValue {
  LoroValue::F64(x)
}

///|
pub fn LoroValue::str(s : String) -> LoroValue {
  LoroValue::Str(s)
}

///|
pub fn LoroValue::binary(b : Bytes) -> LoroValue {
  LoroValue::Binary(b)
}

///|
pub fn LoroValue::list(items : Array[LoroValue]) -> LoroValue {
  LoroValue::List(items)
}

///|
pub fn LoroValue::map(items : Array[(UInt64, LoroValue)]) -> LoroValue {
  LoroValue::Map(items)
}

///|
pub fn LoroValue::container_type(kind : Byte) -> LoroValue {
  LoroValue::ContainerType(kind)
}

///|
pub enum Value {
  Null
  True
  False
  I64(Int64)
  F64(Double)
  Str(String)
  Binary(Bytes)
  ContainerIdx(UInt64)
  DeleteOnce
  DeleteSeq
  DeltaInt(Int)
  LoroValue(LoroValue)
  MarkStart(MarkStart)
  TreeMove(TreeMove)
  ListMove(ListMove)
  ListSet(ListSet)
  RawTreeMove(RawTreeMove)
  Future(Byte, Bytes) // (raw tag byte, binary payload bytes)
} derive(Eq, Show)

///|
pub struct MarkStart {
  info : Byte
  len : UInt64
  key_idx : UInt64
  value : LoroValue
} derive(Eq, Show)

///|
pub struct TreeMove {
  target_idx : UInt64
  is_parent_null : Bool
  position : UInt64
  parent_idx : UInt64?
} derive(Eq, Show)

///|
pub struct ListMove {
  from : UInt64
  from_idx : UInt64
  lamport : UInt64
} derive(Eq, Show)

///|
pub struct ListSet {
  peer_idx : UInt64
  lamport : UInt
  value : LoroValue
} derive(Eq, Show)

///|
pub struct RawTreeMove {
  subject_peer_idx : UInt64
  subject_cnt : Int
  position_idx : UInt64
  is_parent_null : Bool
  parent_peer_idx : UInt64
  parent_cnt : Int
} derive(Eq, Show)
