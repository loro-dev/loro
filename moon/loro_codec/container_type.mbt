///|
pub enum ContainerType {
  Text
  Map
  List
  MovableList
  Tree
  Counter
  Unknown(UInt)
} derive(Eq, Show)

// NOTE: MoonBit makes enum constructors read-only across packages. Expose explicit
// constructor functions for blackbox tests and future callers.

///|
pub fn ContainerType::text() -> ContainerType {
  ContainerType::Text
}

///|
pub fn ContainerType::map() -> ContainerType {
  ContainerType::Map
}

///|
pub fn ContainerType::list() -> ContainerType {
  ContainerType::List
}

///|
pub fn ContainerType::movable_list() -> ContainerType {
  ContainerType::MovableList
}

///|
pub fn ContainerType::tree() -> ContainerType {
  ContainerType::Tree
}

///|
pub fn ContainerType::counter() -> ContainerType {
  ContainerType::Counter
}

///|
pub fn ContainerType::unknown(kind : UInt) -> ContainerType {
  ContainerType::Unknown(kind)
}

///|
pub fn container_type_to_u8(t : ContainerType) -> Byte {
  match t {
    ContainerType::Map => b'\x00'
    ContainerType::List => b'\x01'
    ContainerType::Text => b'\x02'
    ContainerType::Tree => b'\x03'
    ContainerType::MovableList => b'\x04'
    ContainerType::Counter => b'\x05'
    ContainerType::Unknown(k) => (k & 0xFF).to_byte()
  }
}

///|
pub fn container_type_from_u8(b : Byte) -> ContainerType {
  match b.to_uint() {
    0 => ContainerType::Map
    1 => ContainerType::List
    2 => ContainerType::Text
    3 => ContainerType::Tree
    4 => ContainerType::MovableList
    5 => ContainerType::Counter
    x => ContainerType::Unknown(x)
  }
}

// Used by serde (postcard) for historical reasons.

///|
pub fn container_type_to_historical_u8(t : ContainerType) -> Byte {
  match t {
    ContainerType::Text => b'\x00'
    ContainerType::Map => b'\x01'
    ContainerType::List => b'\x02'
    ContainerType::MovableList => b'\x03'
    ContainerType::Tree => b'\x04'
    ContainerType::Counter => b'\x05'
    ContainerType::Unknown(k) => (k & 0xFF).to_byte()
  }
}

///|
pub fn container_type_from_historical_u8(b : Byte) -> ContainerType {
  match b.to_uint() {
    0 => ContainerType::Text
    1 => ContainerType::Map
    2 => ContainerType::List
    3 => ContainerType::MovableList
    4 => ContainerType::Tree
    5 => ContainerType::Counter
    x => ContainerType::Unknown(x)
  }
}
