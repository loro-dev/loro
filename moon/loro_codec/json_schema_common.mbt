///|
const LORO_CONTAINER_ID_PREFIX : String = "🦜:"

///|
fn id_inc(id : ID, delta : Int) -> ID raise DecodeError {
  let mut counter64 = id.counter().to_int64()
  counter64 = counter64 + delta.to_int64()
  if counter64 < -2147483648L || counter64 > 2147483647L {
    raise DecodeError("jsonschema: id counter overflow")
  }
  ID::new(id.peer(), counter64.to_int())
}
