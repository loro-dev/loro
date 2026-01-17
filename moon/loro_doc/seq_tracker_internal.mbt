fn list_values_of_tracker(t : SeqTracker) -> Array[String] {
  let out : Array[String] = []
  for s in t.rope.spans {
    if s.status.is_activated() {
      match s.content {
        SeqContent::List(items) =>
          for v in items {
            match v {
              Value::Str(x) => out.push(x)
              _ => out.push("<non-str>")
            }
          }
        _ => ()
      }
    }
  }
  out
}

fn list_values_of_deltas(deltas : Array[SeqDelta]) -> Array[String] {
  let out : Array[String] = []
  for d in deltas {
    match d {
      SeqDelta::Insert(ins) =>
        match ins.content {
          SeqContent::List(items) =>
            for v in items {
              match v {
                Value::Str(x) => out.push(x)
                _ => out.push("<non-str>")
              }
            }
          _ => ()
        }
      _ => ()
    }
  }
  out
}

///|
test "seq tracker concurrent inserts converge by peer" {
  let vv_empty : @hashmap.HashMap[UInt64, Int] = @hashmap.new(capacity=0)
  let vv_merged : @hashmap.HashMap[UInt64, Int] = @hashmap.new(capacity=4)
  vv_merged.set(1UL, 1)
  vv_merged.set(2UL, 1)

  let t_ab = SeqTracker::new()
  try! t_ab.insert(
    SeqIdFull::new(1UL, 0, 0U),
    0,
    SeqContent::List([Value::str("A")]),
  )
  try! t_ab.checkout(vv_empty)
  try! t_ab.insert(
    SeqIdFull::new(2UL, 0, 0U),
    0,
    SeqContent::List([Value::str("B")]),
  )
  try! t_ab.checkout(vv_merged)

  let t_ba = SeqTracker::new()
  try! t_ba.insert(
    SeqIdFull::new(2UL, 0, 0U),
    0,
    SeqContent::List([Value::str("B")]),
  )
  try! t_ba.checkout(vv_empty)
  try! t_ba.insert(
    SeqIdFull::new(1UL, 0, 0U),
    0,
    SeqContent::List([Value::str("A")]),
  )
  try! t_ba.checkout(vv_merged)

  let ab = list_values_of_tracker(t_ab)
  let ba = list_values_of_tracker(t_ba)
  assert_eq(ab, ["A", "B"])
  assert_eq(ba, ["A", "B"])
}

///|
test "seq tracker diff emits created inserts" {
  let vv_empty : @hashmap.HashMap[UInt64, Int] = @hashmap.new(capacity=0)
  let vv_merged : @hashmap.HashMap[UInt64, Int] = @hashmap.new(capacity=4)
  vv_merged.set(1UL, 1)
  vv_merged.set(2UL, 1)

  let t = SeqTracker::new()
  try! t.insert(
    SeqIdFull::new(1UL, 0, 0U),
    0,
    SeqContent::List([Value::str("A")]),
  )
  try! t.checkout(vv_empty)
  try! t.insert(
    SeqIdFull::new(2UL, 0, 0U),
    0,
    SeqContent::List([Value::str("B")]),
  )
  // diff should not require current state to be at `to`
  let deltas = try! t.diff(vv_empty, vv_merged)
  let inserts = list_values_of_deltas(deltas)
  assert_eq(inserts, ["A", "B"])
}
