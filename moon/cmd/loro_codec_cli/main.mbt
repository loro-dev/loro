///|
fn usage() -> Unit {
  println("loro-codec (moonbit) v" + @loro_codec.version())
  println("Usage:")
  println("  loro-codec transcode <in.blob> <out.blob>")
  println("  loro-codec decode-updates <in.blob>")
  println("  loro-codec export-jsonschema <in.blob>")
  println("  loro-codec encode-jsonschema <in.json> <out.blob>")
  println("  loro-codec export-deep-json <snapshot.blob>")
}

///|
fn find_cmd_index(args : Array[String]) -> Int? {
  let mut i = 0
  while i < args.length() {
    match args[i] {
      "transcode" => return Some(i)
      "decode-updates" => return Some(i)
      "export-jsonschema" => return Some(i)
      "encode-jsonschema" => return Some(i)
      "export-deep-json" => return Some(i)
      _ => ()
    }
    i = i + 1
  }
  None
}

///|
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
          let output = @loro_codec.transcode_document(input, true) catch {
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
          let json = @loro_codec.decode_fast_updates_changes_json(input, true) catch {
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
          let json = @loro_codec.export_json_schema_from_fast_updates(
            input, true,
          ) catch {
            @loro_codec.DecodeError(msg) => {
              println("decode error: " + msg)
              exit_process(2)
              ""
            }
          }
          println(json)
        }
        "encode-jsonschema" => {
          if i + 2 >= args.length() {
            usage()
            exit_process(1)
          }
          let input_path = args[i + 1]
          let output_path = args[i + 2]
          let input = read_file(input_path)
          let json = @encoding/utf8.decode(input) catch {
            @encoding/utf8.Malformed(_) => {
              println("decode error: invalid utf8 input json")
              exit_process(2)
              ""
            }
          }
          let output = @loro_codec.encode_fast_updates_from_json_schema(
            json, true,
          ) catch {
            @loro_codec.DecodeError(msg) => {
              println("decode error: " + msg)
              exit_process(2)
              b""
            }
          }
          write_file(output_path, output)
        }
        "export-deep-json" => {
          if i + 1 >= args.length() {
            usage()
            exit_process(1)
          }
          let input_path = args[i + 1]
          let input = read_file(input_path)
          let json = @loro_codec.export_deep_json_from_fast_snapshot(
            input, true,
          ) catch {
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
