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

