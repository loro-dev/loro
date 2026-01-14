pub fn read_file(path: String) -> Bytes {
  read_file_internal(path)
}

pub fn write_file(path: String, data: Bytes) -> Unit {
  write_file_internal(path, data)
}

pub fn exit_process(code: Int) -> Unit {
  exit_process_internal(code)
}

