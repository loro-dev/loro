test "container wrapper encode/parse header" {
  let payload = b"PAYLOAD"
  let parent = Some(ContainerID::normal(7, 42, ContainerType::list()))
  let bytes = encode_container_wrapper(ContainerType::text(), 3, parent, payload)

  let parsed = try! parse_container_wrapper(bytes[:])
  assert_eq(parsed.kind(), ContainerType::text())
  assert_eq(parsed.depth(), 3)
  match parsed.parent() {
    Some(ContainerID::Normal(peer, counter, kind)) => {
      assert_eq(peer, 7)
      assert_eq(counter, 42)
      assert_eq(kind, ContainerType::list())
    }
    _ => assert_true(false)
  }
  assert_eq(parsed.payload_view().to_bytes(), payload)
}
