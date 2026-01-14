pub struct Change {
  id: ID
  timestamp: Int64
  deps: Array[ID]
  lamport: UInt
  msg: String?
  ops: Array[Op]
} derive(Eq, Show)

pub fn Change::new(
  id: ID,
  timestamp: Int64,
  deps: Array[ID],
  lamport: UInt,
  msg: String?,
) -> Change {
  { id, timestamp, deps, lamport, msg, ops: [] }
}

pub fn Change::id(self: Change) -> ID {
  self.id
}

pub fn Change::timestamp(self: Change) -> Int64 {
  self.timestamp
}

pub fn Change::deps(self: Change) -> Array[ID] {
  self.deps
}

pub fn Change::lamport(self: Change) -> UInt {
  self.lamport
}

pub fn Change::msg(self: Change) -> String? {
  self.msg
}

pub fn Change::ops(self: Change) -> Array[Op] {
  self.ops
}

