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
