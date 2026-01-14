pub struct IdLp {
  peer: UInt64
  lamport: UInt
} derive(Eq, Show)

pub fn IdLp::new(peer: UInt64, lamport: UInt) -> IdLp {
  { peer, lamport }
}

pub fn IdLp::peer(self: IdLp) -> UInt64 {
  self.peer
}

pub fn IdLp::lamport(self: IdLp) -> UInt {
  self.lamport
}

pub struct FractionalIndex {
  bytes: Bytes
} derive(Eq, Show)

pub fn FractionalIndex::new(bytes: Bytes) -> FractionalIndex {
  { bytes, }
}

pub fn FractionalIndex::bytes(self: FractionalIndex) -> Bytes {
  self.bytes
}

pub enum ListOp {
  Insert(UInt, Array[LoroValue])
  Delete(Int, Int64, ID) // (pos, len, start_id)
} derive(Eq, Show)

pub enum MovableListOp {
  Insert(UInt, Array[LoroValue])
  Delete(Int, Int64, ID) // (pos, len, start_id)
  Move(UInt, UInt, IdLp) // (from, to, elem_id)
  Set(IdLp, LoroValue)
} derive(Eq, Show)

pub enum MapOp {
  Insert(String, LoroValue)
  Delete(String)
} derive(Eq, Show)

pub enum TextOp {
  Insert(UInt, String)
  Delete(Int, Int64, ID) // (pos, len, start_id)
  Mark(UInt, UInt, String, LoroValue, Byte) // (start,end,key,value,info)
  MarkEnd
} derive(Eq, Show)

pub enum TreeOp {
  Create(ID, ID?, FractionalIndex)
  Move(ID, ID?, FractionalIndex)
  Delete(ID)
} derive(Eq, Show)

pub enum FutureOp {
  Unknown(Int, Value) // (prop, raw encoded value)
} derive(Eq, Show)

pub enum OpContent {
  List(ListOp)
  MovableList(MovableListOp)
  Map(MapOp)
  Text(TextOp)
  Tree(TreeOp)
  Future(FutureOp)
} derive(Eq, Show)

pub struct Op {
  container: ContainerID
  counter: Int
  len: UInt
  content: OpContent
} derive(Eq, Show)

pub fn Op::new(container: ContainerID, counter: Int, len: UInt, content: OpContent) -> Op {
  { container, counter, len, content }
}

pub fn Op::container(self: Op) -> ContainerID {
  self.container
}

pub fn Op::counter(self: Op) -> Int {
  self.counter
}

pub fn Op::len(self: Op) -> UInt {
  self.len
}

pub fn Op::content(self: Op) -> OpContent {
  self.content
}
