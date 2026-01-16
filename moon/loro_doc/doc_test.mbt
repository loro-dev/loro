///|
test "lorodoc map set/get is visible before commit" {
  let doc = LoroDoc::new()
  try! doc.setPeerId(1UL)
  let map = try! doc.getMap("map")
  try! map.set("k", Value::Str("v"))
  match map.get("k") {
    Some(Value::Str(s)) => assert_eq(s, "v")
    _ => assert_true(false)
  }
}

///|
test "lorodoc export/update roundtrip (map)" {
  let doc1 = LoroDoc::new()
  try! doc1.setPeerId(1UL)
  let m1 = try! doc1.getMap("map")
  try! m1.set("k", Value::Str("v"))

  let bytes = try! doc1.export({ from: None })
  let parsed = try! @loro_codec.parse_document(bytes, true)
  assert_eq(parsed.mode(), 4U)
  let blocks = try! @loro_codec.parse_fast_updates_body(parsed.body_view())
  assert_true(blocks.length() > 0)
  let changes = try! @loro_codec.decode_change_block(blocks[0])
  assert_true(changes.length() > 0)

  let doc2 = LoroDoc::new()
  try! doc2.setPeerId(2UL)
  try! doc2.import(bytes)
  let m2 = try! doc2.getMap("map")
  match m2.get("k") {
    Some(Value::Str(s)) => assert_eq(s, "v")
    _ => assert_true(false)
  }
}

///|
test "lorodoc export/update roundtrip (list)" {
  let doc1 = LoroDoc::new()
  try! doc1.setPeerId(1UL)
  let l1 = try! doc1.getList("list")
  try! l1.push(Value::Str("a"))
  try! l1.push(Value::Str("b"))
  try! l1.delete(0, 1)
  assert_eq(l1.toArray(), [Value::Str("b")])

  let bytes = try! doc1.export({ from: None })
  let doc2 = LoroDoc::new()
  try! doc2.setPeerId(2UL)
  try! doc2.import(bytes)
  let l2 = try! doc2.getList("list")
  assert_eq(l2.toArray(), [Value::Str("b")])
}

///|
test "lorodoc export/update roundtrip (text)" {
  let doc1 = LoroDoc::new()
  try! doc1.setPeerId(1UL)
  let t1 = try! doc1.getText("text")
  try! t1.insert(0, "hello")
  try! t1.delete(1, 2)
  assert_eq(t1.toString(), "hlo")

  let bytes = try! doc1.export({ from: None })
  let doc2 = LoroDoc::new()
  try! doc2.setPeerId(2UL)
  try! doc2.import(bytes)
  let t2 = try! doc2.getText("text")
  assert_eq(t2.toString(), "hlo")
}

///|
test "lorodoc concurrent list insert converges" {
  let doc1 = LoroDoc::new()
  try! doc1.setPeerId(1UL)
  let l1 = try! doc1.getList("list")
  try! l1.insert(0, Value::Str("A"))
  let bytes1 = try! doc1.export({ from: None })

  let doc2 = LoroDoc::new()
  try! doc2.setPeerId(2UL)
  let l2 = try! doc2.getList("list")
  try! l2.insert(0, Value::Str("B"))
  let bytes2 = try! doc2.export({ from: None })

  try! doc1.import(bytes2)
  try! doc2.import(bytes1)

  assert_eq(l1.toArray(), [Value::Str("A"), Value::Str("B")])
  assert_eq(l2.toArray(), [Value::Str("A"), Value::Str("B")])
}
