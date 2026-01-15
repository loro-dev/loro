///|
pub fn transcode_container_state_snapshot(
  kind : ContainerType,
  payload : BytesView,
  validate : Bool,
) -> Bytes raise DecodeError {
  match kind {
    ContainerType::Map => transcode_map_state_snapshot(payload, validate)
    ContainerType::List => transcode_list_state_snapshot(payload, validate)
    ContainerType::Text => transcode_richtext_state_snapshot(payload, validate)
    ContainerType::Tree => transcode_tree_state_snapshot(payload, validate)
    ContainerType::MovableList =>
      transcode_movable_list_state_snapshot(payload, validate)
    ContainerType::Counter =>
      transcode_counter_state_snapshot(payload, validate)
    ContainerType::Unknown(_) => payload.to_bytes()
  }
}
