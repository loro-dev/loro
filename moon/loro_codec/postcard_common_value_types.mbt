///|
pub enum CommonValue {
  Null
  Bool(Bool)
  Double(Double)
  I64(Int64)
  String(String)
  List(Array[CommonValue])
  Map(Array[(String, CommonValue)])
  Container(ContainerID)
  Binary(Bytes)
} derive(Eq, Show)

// NOTE: MoonBit makes enum constructors read-only across packages. Expose explicit
// constructor functions for blackbox tests and future callers.

///|
pub fn CommonValue::null() -> CommonValue {
  CommonValue::Null
}

///|
pub fn CommonValue::bool(v : Bool) -> CommonValue {
  CommonValue::Bool(v)
}

///|
pub fn CommonValue::double(v : Double) -> CommonValue {
  CommonValue::Double(v)
}

///|
pub fn CommonValue::i64(v : Int64) -> CommonValue {
  CommonValue::I64(v)
}

///|
pub fn CommonValue::string(v : String) -> CommonValue {
  CommonValue::String(v)
}

///|
pub fn CommonValue::binary(v : Bytes) -> CommonValue {
  CommonValue::Binary(v)
}

///|
pub fn CommonValue::list(v : Array[CommonValue]) -> CommonValue {
  CommonValue::List(v)
}

///|
pub fn CommonValue::map(v : Array[(String, CommonValue)]) -> CommonValue {
  CommonValue::Map(v)
}

///|
pub fn CommonValue::container(v : ContainerID) -> CommonValue {
  CommonValue::Container(v)
}
