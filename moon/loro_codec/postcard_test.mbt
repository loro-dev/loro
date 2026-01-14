test "postcard: id encode/decode" {
  let id = ID::new(7UL, 42)
  let bytes = postcard_encode_id(id)
  let (decoded, rest) = try! postcard_take_id(bytes[:])
  assert_eq(rest.length(), 0)
  assert_eq(decoded, id)
}

test "postcard: container id encode/decode" {
  let cid0 = ContainerID::root("r", ContainerType::map())
  let bytes0 = postcard_encode_container_id(cid0)
  let (decoded0, rest0) = try! postcard_take_container_id(bytes0[:])
  assert_eq(rest0.length(), 0)
  assert_eq(decoded0, cid0)

  let cid1 = ContainerID::normal(7UL, 42, ContainerType::list())
  let bytes1 = postcard_encode_container_id(cid1)
  let (decoded1, rest1) = try! postcard_take_container_id(bytes1[:])
  assert_eq(rest1.length(), 0)
  assert_eq(decoded1, cid1)
}

test "postcard: common value encode/decode basics" {
  let values: Array[CommonValue] = [
    CommonValue::null(),
    CommonValue::bool(false),
    CommonValue::bool(true),
    CommonValue::i64(-1L),
    CommonValue::string("hi"),
    CommonValue::binary(b"\x01\x02"),
    CommonValue::list([CommonValue::null(), CommonValue::i64(1L)]),
    CommonValue::map([("a", CommonValue::null()), ("b", CommonValue::i64(2L))]),
    CommonValue::container(ContainerID::root("x", ContainerType::text())),
  ]

  for v in values {
    let bytes = postcard_encode_common_value(v)
    let (decoded, rest) = try! postcard_take_common_value(bytes[:])
    assert_eq(rest.length(), 0)
    assert_eq(decoded, v)
  }
}

test "postcard: frontiers / version vector encode/decode" {
  let vv: VersionVector = [(1UL, 2), (7UL, 42)]
  let vv_bytes = postcard_encode_version_vector(vv)
  let (vv2, rest0) = try! postcard_take_version_vector(vv_bytes[:])
  assert_eq(rest0.length(), 0)
  assert_eq(vv2, vv)

  let fr: Array[ID] = [ID::new(7UL, 42), ID::new(7UL, 43)]
  let fr_bytes = postcard_encode_frontiers(fr)
  let (fr2, rest1) = try! postcard_take_frontiers(fr_bytes[:])
  assert_eq(rest1.length(), 0)
  assert_eq(fr2, fr)
}
