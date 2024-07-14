---
"loro-wasm": patch
"loro-crdt": patch
---

Better text APIs and bug fixes

### 🚀 Features

- Add insert_utf8 and delete_utf8 for Rust Text API (#396)
- Add text iter (#400)
- Add more text api (#398)

### 🐛 Bug Fixes

- Tree undo when processing deleted node (#399)
- Tree diff calc children should be sorted by idlp (#401)
- When computing the len of the map, do not count elements that are None (#402)

### 📚 Documentation

- Update wasm docs
- Rm experimental warning

### ⚙️ Miscellaneous Tasks

- Update fuzz config
- Pnpm
- Rename position to fractional_index (#381)

