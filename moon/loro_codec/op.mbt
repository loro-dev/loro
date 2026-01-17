///|
pub struct IdLp {
  peer : UInt64
  lamport : UInt
} derive(Eq, Show)

///|
pub fn IdLp::new(peer : UInt64, lamport : UInt) -> IdLp {
  { peer, lamport }
}

///|
pub fn IdLp::peer(self : IdLp) -> UInt64 {
  self.peer
}

///|
pub fn IdLp::lamport(self : IdLp) -> UInt {
  self.lamport
}

///|
pub struct FractionalIndex {
  bytes : Bytes
} derive(Eq, Show)

///|
pub fn FractionalIndex::new(bytes : Bytes) -> FractionalIndex {
  { bytes, }
}

///|
pub fn FractionalIndex::bytes(self : FractionalIndex) -> Bytes {
  self.bytes
}

///|
pub enum ListOp {
  Insert(UInt, Array[LoroValue])
  Delete(Int, Int64, ID) // (pos, len, start_id)
} derive(Eq, Show)

// NOTE: MoonBit makes enum constructors read-only across packages. Expose explicit
// constructor functions for callers (e.g. `loro_doc`).

///|
pub fn ListOp::insert(pos : UInt, values : Array[LoroValue]) -> ListOp {
  ListOp::Insert(pos, values)
}

///|
pub fn ListOp::delete(pos : Int, len : Int64, start_id : ID) -> ListOp {
  ListOp::Delete(pos, len, start_id)
}

///|
pub enum MovableListOp {
  Insert(UInt, Array[LoroValue])
  Delete(Int, Int64, ID) // (pos, len, start_id)
  Move(UInt, UInt, IdLp) // (from, to, elem_id)
  Set(IdLp, LoroValue)
} derive(Eq, Show)

///|
pub fn MovableListOp::insert(pos : UInt, values : Array[LoroValue]) -> MovableListOp {
  MovableListOp::Insert(pos, values)
}

///|
pub fn MovableListOp::delete(pos : Int, len : Int64, start_id : ID) -> MovableListOp {
  MovableListOp::Delete(pos, len, start_id)
}

///|
pub fn MovableListOp::move(from : UInt, to : UInt, elem_id : IdLp) -> MovableListOp {
  MovableListOp::Move(from, to, elem_id)
}

///|
pub fn MovableListOp::set(elem_id : IdLp, value : LoroValue) -> MovableListOp {
  MovableListOp::Set(elem_id, value)
}

///|
pub enum MapOp {
  Insert(String, LoroValue)
  Delete(String)
} derive(Eq, Show)

///|
pub fn MapOp::insert(key : String, value : LoroValue) -> MapOp {
  MapOp::Insert(key, value)
}

///|
pub fn MapOp::delete(key : String) -> MapOp {
  MapOp::Delete(key)
}

///|
pub enum TextOp {
  Insert(UInt, String)
  Delete(Int, Int64, ID) // (pos, len, start_id)
  Mark(UInt, UInt, String, LoroValue, Byte) // (start,end,key,value,info)
  MarkEnd
} derive(Eq, Show)

///|
pub fn TextOp::insert(pos : UInt, text : String) -> TextOp {
  TextOp::Insert(pos, text)
}

///|
pub fn TextOp::delete(pos : Int, len : Int64, start_id : ID) -> TextOp {
  TextOp::Delete(pos, len, start_id)
}

///|
pub fn TextOp::mark(
  start : UInt,
  end : UInt,
  key : String,
  value : LoroValue,
  info : Byte,
) -> TextOp {
  TextOp::Mark(start, end, key, value, info)
}

///|
pub fn TextOp::mark_end() -> TextOp {
  TextOp::MarkEnd
}

///|
pub enum TreeOp {
  Create(ID, ID?, FractionalIndex)
  Move(ID, ID?, FractionalIndex)
  Delete(ID)
} derive(Eq, Show)

///|
pub fn TreeOp::create(target : ID, parent : ID?, position : FractionalIndex) -> TreeOp {
  TreeOp::Create(target, parent, position)
}

///|
pub fn TreeOp::move(target : ID, parent : ID?, position : FractionalIndex) -> TreeOp {
  TreeOp::Move(target, parent, position)
}

///|
pub fn TreeOp::delete(target : ID) -> TreeOp {
  TreeOp::Delete(target)
}

///|
pub enum FutureOp {
  Unknown(Int, Value) // (prop, raw encoded value)
} derive(Eq, Show)

///|
pub fn FutureOp::unknown(prop : Int, value : Value) -> FutureOp {
  FutureOp::Unknown(prop, value)
}

///|
pub enum OpContent {
  List(ListOp)
  MovableList(MovableListOp)
  Map(MapOp)
  Text(TextOp)
  Tree(TreeOp)
  Future(FutureOp)
} derive(Eq, Show)

///|
pub fn OpContent::list(op : ListOp) -> OpContent {
  OpContent::List(op)
}

///|
pub fn OpContent::movable_list(op : MovableListOp) -> OpContent {
  OpContent::MovableList(op)
}

///|
pub fn OpContent::map(op : MapOp) -> OpContent {
  OpContent::Map(op)
}

///|
pub fn OpContent::text(op : TextOp) -> OpContent {
  OpContent::Text(op)
}

///|
pub fn OpContent::tree(op : TreeOp) -> OpContent {
  OpContent::Tree(op)
}

///|
pub fn OpContent::future(op : FutureOp) -> OpContent {
  OpContent::Future(op)
}

///|
pub struct Op {
  container : ContainerID
  counter : Int
  len : UInt
  content : OpContent
} derive(Eq, Show)

///|
pub fn Op::new(
  container : ContainerID,
  counter : Int,
  len : UInt,
  content : OpContent,
) -> Op {
  { container, counter, len, content }
}

///|
pub fn Op::container(self : Op) -> ContainerID {
  self.container
}

///|
pub fn Op::counter(self : Op) -> Int {
  self.counter
}

///|
pub fn Op::len(self : Op) -> UInt {
  self.len
}

///|
pub fn Op::content(self : Op) -> OpContent {
  self.content
}
