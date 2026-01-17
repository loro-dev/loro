///|
test "internal smoke: list/text basic ops" {
  let doc = LoroDoc::new()
  try! doc.setPeerId(1UL)

  let l = try! doc.getList("list")
  try! l.push(Value::str("a"))
  try! l.push(Value::str("b"))
  assert_eq(l.toArray().length(), 2)
  try! l.delete(0, 1)
  assert_eq(l.toArray(), [Value::str("b")])

  let t = try! doc.getText("text")
  try! t.insert(0, "hello")
  try! t.delete(1, 2)
  assert_eq(t.toString(), "hlo")
}
