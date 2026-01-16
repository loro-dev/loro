## Conditional Compilation

Target specific backends/modes in `moon.pkg.json`:

```json
{
  "targets": {
    "wasm_only.mbt": ["wasm"],
    "js_only.mbt": ["js"],
    "debug_only.mbt": ["debug"],
    "wasm_or_js.mbt": ["wasm", "js"], // for wasm or js backend
    "not_js.mbt": ["not", "js"], // for nonjs backend
    "complex.mbt": ["or", ["and", "wasm", "release"], ["and", "js", "debug"]] // more complex conditions
  }
}
```

**Available conditions:**

- **Backends**: `"wasm"`, `"wasm-gc"`, `"js"`, `"native"`
- **Build modes**: `"debug"`, `"release"`
- **Logical operators**: `"and"`, `"or"`, `"not"`

## Link Configuration

### Basic Linking

```json
{
  "link": true, // Enable linking for this package
  // OR for advanced cases:
  "link": {
    "wasm": {
      "exports": ["hello", "foo:bar"], // Export functions
      "heap-start-address": 1024, // Memory layout
      "import-memory": {
        // Import external memory
        "module": "env",
        "name": "memory"
      },
      "export-memory-name": "memory" // Export memory with name
    },
    "wasm-gc": {
      "exports": ["hello"],
      "use-js-builtin-string": true, // JS String Builtin support
      "imported-string-constants": "_" // String namespace
    },
    "js": {
      "exports": ["hello"],
      "format": "esm" // "esm", "cjs", or "iife"
    },
    "native": {
      "cc": "gcc", // C compiler
      "cc-flags": "-O2 -DMOONBIT", // Compile flags
      "cc-link-flags": "-s" // Link flags
    }
  }
}
```

## Warning Control

Disable specific warnings in `moon.mod.json` or `moon.pkg.json`:

```json
{
  "warn-list": "-2-29" // Disable unused variable (2) & unused package (29)
}
```

**Common warning numbers:**

- `1` - Unused function
- `2` - Unused variable
- `11` - Partial pattern matching
- `12` - Unreachable code
- `29` - Unused package

Use `moonc build-package -warn-help` to see all available warnings.

## Pre-build Commands

Embed external files as MoonBit code:

```json
{
  "pre-build": [
    {
      "input": "data.txt",
      "output": "embedded.mbt",
      "command": ":embed -i $input -o $output --name data --text"
    },
    ... // more embed commands
  ]
}
```

Generated code example:

```mbt check
///|
let data : String =
  #|hello,
  #|world
  #|
```
