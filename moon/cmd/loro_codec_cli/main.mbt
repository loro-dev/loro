fn usage() -> Unit {
  println("loro-codec (moonbit) v" + @loro_codec.version())
  println("Usage:")
  println("  loro-codec transcode <in.blob> <out.blob>")
  println("  loro-codec decode-updates <in.blob>")
  println("  loro-codec export-jsonschema <in.blob>")
}

fn find_cmd_index(args: Array[String]) -> Int? {
  let mut i = 0
  while i < args.length() {
    match args[i] {
      "transcode" => return Some(i)
      "decode-updates" => return Some(i)
      "export-jsonschema" => return Some(i)
      _ => ()
    }
    i = i + 1
  }
  None
}

fn main {
  let args = @env.args()
  match find_cmd_index(args) {
    None => {
      usage()
      exit_process(1)
    }
    Some(i) => {
      let cmd = args[i]
      match cmd {
        "transcode" => {
          if i + 2 >= args.length() {
            usage()
            exit_process(1)
          }
          let input_path = args[i + 1]
          let output_path = args[i + 2]
          let input = read_file(input_path)
          let output = try
            @loro_codec.transcode_document(input, true)
          catch {
            @loro_codec.DecodeError(msg) => {
              println("decode error: " + msg)
              exit_process(2)
              b""
            }
          }
          write_file(output_path, output)
        }
        "decode-updates" => {
          if i + 1 >= args.length() {
            usage()
            exit_process(1)
          }
          let input_path = args[i + 1]
          let input = read_file(input_path)
          let json = try
            @loro_codec.decode_fast_updates_changes_json(input, true)
          catch {
            @loro_codec.DecodeError(msg) => {
              println("decode error: " + msg)
              exit_process(2)
              ""
            }
          }
          println(json)
        }
        "export-jsonschema" => {
          if i + 1 >= args.length() {
            usage()
            exit_process(1)
          }
          let input_path = args[i + 1]
          let input = read_file(input_path)
          let json = try
            @loro_codec.export_json_schema_from_fast_updates(input, true)
          catch {
            @loro_codec.DecodeError(msg) => {
              println("decode error: " + msg)
              exit_process(2)
              ""
            }
          }
          println(json)
        }
        _ => {
          usage()
          exit_process(1)
        }
      }
    }
  }
}
