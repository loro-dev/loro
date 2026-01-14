fn read_file_internal(_path: String) -> Bytes {
  abort("read_file is not supported on native target yet")
}

fn write_file_internal(_path: String, _data: Bytes) -> Unit {
  abort("write_file is not supported on native target yet")
}

fn exit_process_internal(_code: Int) -> Unit {
  abort("exit_process is not supported on native target yet")
}
