///|
extern "js" fn read_file_internal(path: String) -> Bytes =
  #| function(path) {
  #|   const fs = require("node:fs");
  #|   return fs.readFileSync(path);
  #| }

///|
extern "js" fn write_file_internal(path: String, data: Bytes) -> Unit =
  #| function(path, data) {
  #|   const fs = require("node:fs");
  #|   fs.writeFileSync(path, data);
  #| }

///|
extern "js" fn exit_process_internal(code: Int) -> Unit =
  #| function(code) {
  #|   if (typeof process !== "undefined" && typeof process.exit === "function") {
  #|     process.exit(code);
  #|   }
  #|   throw new Error("process.exit is not available");
  #| }

