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

///|
test "lorodoc export/update roundtrip (movable_list)" {
  let doc1 = LoroDoc::new()
  try! doc1.setPeerId(1UL)
  let ml1 = try! doc1.getMovableList("ml")
  try! ml1.push(Value::Str("a"))
  try! ml1.push(Value::Str("b"))
  try! ml1.push(Value::Str("c"))
  try! ml1.move(0, 2)
  try! ml1.set(1, Value::Str("B"))
  try! ml1.delete(0, 1)
  assert_eq(ml1.toArray(), [Value::Str("c"), Value::Str("B")])

  let bytes = try! doc1.export({ from: None })
  let doc2 = LoroDoc::new()
  try! doc2.setPeerId(2UL)
  try! doc2.import(bytes)
  let ml2 = try! doc2.getMovableList("ml")
  assert_eq(ml2.toArray(), [Value::Str("c"), Value::Str("B")])
}

///|
test "lorodoc concurrent movable_list move converges (lww)" {
  let a = LoroDoc::new()
  try! a.setPeerId(1UL)
  let ml_a = try! a.getMovableList("ml")
  try! ml_a.push(Value::Str("a"))
  try! ml_a.push(Value::Str("b"))
  try! ml_a.push(Value::Str("c"))
  let base = try! a.export({ from: None })
  let vv_base = a.oplogVersion()

  let b = LoroDoc::new()
  try! b.setPeerId(2UL)
  try! b.import(base)
  let ml_b = try! b.getMovableList("ml")

  // Concurrent moves of the same element; higher (lamport, peer) wins.
  try! ml_a.move(0, 2)
  try! ml_b.move(0, 1)
  let upd_a = try! a.export({ from: Some(vv_base) })
  let upd_b = try! b.export({ from: Some(vv_base) })
  try! a.import(upd_b)
  try! b.import(upd_a)

  assert_eq(ml_a.toArray(), [Value::Str("b"), Value::Str("a"), Value::Str("c")])
  assert_eq(ml_b.toArray(), [Value::Str("b"), Value::Str("a"), Value::Str("c")])
}

///|
test "lorodoc concurrent movable_list set converges (lww)" {
  let a = LoroDoc::new()
  try! a.setPeerId(1UL)
  let ml_a = try! a.getMovableList("ml")
  try! ml_a.push(Value::Str("a"))
  let base = try! a.export({ from: None })
  let vv_base = a.oplogVersion()

  let b = LoroDoc::new()
  try! b.setPeerId(2UL)
  try! b.import(base)
  let ml_b = try! b.getMovableList("ml")

  try! ml_a.set(0, Value::Str("x"))
  try! ml_b.set(0, Value::Str("y"))
  let upd_a = try! a.export({ from: Some(vv_base) })
  let upd_b = try! b.export({ from: Some(vv_base) })
  try! a.import(upd_b)
  try! b.import(upd_a)

  assert_eq(ml_a.toArray(), [Value::Str("y")])
  assert_eq(ml_b.toArray(), [Value::Str("y")])
}

///|
test "lorodoc setContainer/insertContainer attaches nested containers" {
  let doc1 = LoroDoc::new()
  try! doc1.setPeerId(1UL)

  let root = try! doc1.getMap("root")
  let c_text = try! root.setContainer(
    "text",
    Container::detached(@loro_codec.ContainerType::text()),
  )
  let text = try! c_text.asText()
  try! text.insert(0, "hi")
  let expected_root = @json.parse("{\"text\":\"hi\"}"[:]) catch { _ => fail("invalid json") }
  assert_eq(root.toJSON(), expected_root)

  let list = try! doc1.getList("list")
  let c_list_text = try! list.insertContainer(
    0,
    Container::detached(@loro_codec.ContainerType::text()),
  )
  let list_text = try! c_list_text.asText()
  try! list_text.insert(0, "L")
  let expected_list = @json.parse("[\"L\"]"[:]) catch { _ => fail("invalid json") }
  assert_eq(list.toJSON(), expected_list)

  let ml = try! doc1.getMovableList("ml")
  let c_ml_text = try! ml.insertContainer(
    0,
    Container::detached(@loro_codec.ContainerType::text()),
  )
  let ml_text = try! c_ml_text.asText()
  try! ml_text.insert(0, "M")
  let expected_ml = @json.parse("[\"M\"]"[:]) catch { _ => fail("invalid json") }
  assert_eq(ml.toJSON(), expected_ml)

  let bytes = try! doc1.export({ from: None })
  let doc2 = LoroDoc::new()
  try! doc2.setPeerId(2UL)
  try! doc2.import(bytes)
  let root2 = try! doc2.getMap("root")
  assert_eq(root2.toJSON(), expected_root)
  let list2 = try! doc2.getList("list")
  assert_eq(list2.toJSON(), expected_list)
  let ml2 = try! doc2.getMovableList("ml")
  assert_eq(ml2.toJSON(), expected_ml)
}
