---
name: moonbit-agent-guide
description: Guide for writing, refactoring, and testing MoonBit projects. Use when working in MoonBit modules or packages, organizing MoonBit files, using moon tooling (build/check/test/doc/ide), or following MoonBit-specific layout, documentation, and testing conventions.
---

# MoonBit Project Layouts

MoonBit use the `.mbt` extension and interface files `.mbti`. At
the top-level of a MoonBit project there is a `moon.mod.json` file specifying
the metadata of the project. The project may contain multiple packages, each
with its own `moon.pkg.json` file.

## Example layout

```
my_module
├── moon.mod.json             # Module metadata, source field(optional) specifies the source directory of the module
├── moon.pkg.json             # Package metadata (each directory is a package like Golang)
├── README.mbt.md             # Markdown with tested code blocks (`test "..." { ... }`)
├── README.md -> README.mbt.md
├── cmd                       # Command line directory
│   └── main
│       ├── main.mbt
│       └── moon.pkg.json     # executable package with {"is_main": true}
├── liba/                     # Library packages
│   └── moon.pkg.json         # Referenced by other packages as `@username/my_module/liba`
│   └── libb/                 # Library packages
│       └── moon.pkg.json     # Referenced by other packages as `@username/my_module/liba/libb`
├── user_pkg.mbt              # Root packages, referenced by other packages as `@username/my_module`
├── user_pkg_wbtest.mbt       # White-box tests (only needed for testing internal private members, similar to Golang's package mypackage)
└── user_pkg_test.mbt         # Black-box tests
└── ...                       # More package files, symbols visible to current package (like Golang)
```

- **Module**:  `moon.mod.json` file in the project directory.
  A MoonBit *module* is like a Go module,it is a collection of packages in subdirectories, usually corresponding to a repository or project.
  Module boundaries matter for dependency management and import paths.

- **Package**: a `moon.pkg.json` file per directory. 
  All subcommands of `moon` will
  still be executed in the directory of the module (where `moon.mod.json` is
  located), not the current package.
  A MoonBit *package* is the actual compilation unit (like a Go package).
  All source files in the same package are concatenated into one unit.
  The `package` name in the source defines the package, not the file name.
  Imports refer to module + package paths, NEVER to file names.

- **Files**:
  A `.mbt` file is just a chunk of source inside a package.
  File names do NOT create modules or namespaces.
  You may freely split/merge/move declarations between files in the same package.
  Any declaration in a package can reference any other declaration in that package, regardless of file.


## Coding/layout rules you MUST follow:

1. Prefer many small, cohesive files over one large file.
   - Group related types and functions into focused files (e.g. http_client.mbt, router.mbt).
   - If a file is getting large or unfocused, create a new file and move related declarations into it.

2. You MAY freely move declarations between files inside the same package.
   - Each block is separated by `///|`, moving a function/struct/trait between files does not change semantics, as long as its name and pub-ness stay the same, the order of each block is irrelevant too.
   - It is safe to refactor by splitting or merging files inside a package.

3. File names are purely organizational.
   - Do NOT assume file names define modules, and do NOT use file names in type paths.
   - Choose file names to describe a feature or responsibility, not to mirror type names rigidly.

4. When adding new code:
   - Prefer adding it to an existing file that matches the feature.
   - If no good file exists, create a new file under the same package with a descriptive name.
   - Avoid creating giant “misc” or “util” files.

5. Tests:
   - Place tests in dedicated test files (e.g. *_test.mbt) within the appropriate package.
     For a package, besides `*_test.mbt`files,`*.mbt.md`are also blackbox test files, the code block `mbt check` are treated as test cases, they serve both purposes: documentation and tests.      
     You may have `README.mbt.md` files with `mbt check` code examples, you can also symlink `README.mbt.md` to `README.md`
     to make it integrate better with GitHub.
   - It is fine—and encouraged—to have multiple small test files.

6. Interface files(`pkg.generated.mbti`) 
   `pkg.generated.mbti` is compiler-generated summaries of each package's public API surface. They provide a formal, concise overview of all exported types, functions, and traits without implementation details.
   They are generated using `moon info`, useful for code review, when you have a commit that does not change public APIs, `pkg.generated.mbti` files will remain unchanged, so it is recommended to put `pkg.generated.mbti` in version control when you are done.
   
   You can also use `moon doc @moonbitlang/core/strconv` to explore the public API of a package interactively and `moon ide peek-def 'Array::join'` to read
   the definition.

# Common Pitfalls to Avoid

- **Don't use uppercase for variables/functions** - compilation error
- **Don't forget `mut` for mutable record fields** - immutable by default
- **Don't ignore error handling** - errors must be explicitly handled
- **Don't use `return` unnecessarily** - last expression is the return value
- **Don't create methods without Type:: prefix** - methods need explicit type prefix
- **Don't forget to handle array bounds** - use `get()` for safe access
- **Don't forget @package prefix when calling functions from other packages**
- **Don't use ++ or -- (not supported)** - use `i = i + 1` or `i += 1`
- **Don't add explicit `try` for error-raising functions** - errors propagate automatically (unlike Swift)
- **Legacy syntax**: Older code may use `function_name!(...)` or `function_name(...)?` - these are deprecated; use normal calls and `try?` for Result conversion


# `moon` Essentials

## Essential Commands

- `moon new my_project` - Create new project
- `moon run cmd/main` - Run main package
- `moon build` - Build project
- `moon check` - Type check without building, use it REGULARLY, it is fast
- `moon info` - Type check and generate `mbti` files
  run it to see if any public interfaces changed.
- `moon check --target all` - Type check for all backends
- `moon add package` - Add dependency
- `moon remove package` - Remove dependency
- `moon fmt` - Format code

### Test Commands

- `moon test` - Run all tests
- `moon test --update` - Update snapshots
- `moon test -v` - Verbose output with test names
- `moon test [dirname|filename]` - Test specific directory or file
- `moon coverage analyze` - Analyze coverage
- `moon test --filter 'globl'` - Run tests matching filter
  ```
  moon test float/float_test.mbt --filter "Float::*"
  ```
## `README.mbt.md` Generation Guide

- Output `README.mbt.md` in the package directory. 
  `*.mbt.md` file and docstring contents treats `mbt check` specially.
  `mbt check` block will be included directly as code and also run by `moon check` and `moon test`.  If you don't want the code snippets to be checked, explicit `mbt nocheck` is preferred.
  If you are only referencing types from the package, you should use `mbt nocheck` which will only be syntax highlighted.
  Symlink `README.mbt.md` to `README.md` to adapt to systems that expect `README.md`. 
    
## Testing Guide

Use snapshot tests as it is easy to update when behavior changes.

- **Snapshot Tests**: `inspect(value, content="...")`. If unknown, write `inspect(value)` and run `moon test --update` (or `moon test -u`).
  - Use regular `inspect()` for simple values (uses `Show` trait)
  - Use `@json.inspect()` for complex nested structures (uses `ToJson` trait, produces more readable output)
  - It is encouraged to `inspect` or `@json.inspect` the whole return value of a function if
    the whole return value is not huge, this makes test simple. You need `impl (Show|ToJson) for YourType` or `derive (Show, ToJson)`.
- **Update workflow**: After changing code that affects output, run `moon test --update` to regenerate snapshots, then review the diffs in your test files (the `content=` parameter will be updated automatically).

- Black-box by default: Call only public APIs via `@package.fn`. Use white-box tests only when private members matter.
- Grouping: Combine related checks in one `test "..." { ... }` block for speed and clarity.
- Panics: Name test with prefix `test "panic ..." {...}`; if the call returns a value, wrap it with `ignore(...)` to silence warnings.
- Errors: Use `try? f()` to get `Result[...]` and `inspect` it when a function may raise.
- Verify: Run `moon test` (or `-u` to update snapshots) and `moon fmt` afterwards.

### Docstring tests 

Public APIs are encouraged to have docstring tests.
````mbt check
///|
/// Get the largest element of a non-empty `Array`.
///
/// # Example
/// ```mbt check
/// test {
///   inspect(sum_array([1, 2, 3, 4, 5, 6]), content="21")
/// }
/// ```
///
/// # Panics
/// Panics if the `xs` is empty.
pub fn sum_array(xs : Array[Int]) -> Int {
  xs.fold(init=0, (a, b) => a + b)
}
````

The MoonBit code in docstring will be type checked and tested automatically.
(using `moon test --update`). In docstrings, `mbt check` should only contain `test` or `async test`.

## Spec-driven Development

- The spec can be written in a readonly `spec.mbt` file (name is conventional, not mandatory) with stub code marked as declarations:

```mbt check
///|
#declaration_only
pub type Yaml

///|
#declaration_only
pub fn Yaml::to_string(y : Yaml) -> String raise {
  ...
}

///|
#declaration_only
pub fn parse_yaml(s : String) -> Yaml raise {
  ...
}
```

- Add `spec_easy_test.mbt`, `spec_difficult_test.mbt` etc to test the spec functions; everything will be type-checked(`moon check`).
- The AI or students can implement the `declaration_only` functions in different files thanks to our package organization.
- Run `moon test` to check everything is correct.

- `#declaration_only` is supported for functions, methods, and types.
- The `pub type Yaml` line is an intentionally opaque placeholder; the implementer chooses its representation.
- Note the spec file can also contain normal code, not just declarations.

## `moon doc` for API Discovery

**CRITICAL**: `moon doc '<query>'` is your PRIMARY tool for discovering available APIs, functions, types, and methods in MoonBit. Always prefer `moon doc` over other approaches when exploring what APIs are available, it is **more powerful and accurate** than `grep_search` or any regex-based searching tools. 


`moon doc` uses a specialized query syntax designed for symbol lookup:
- **Empty query**: `moon doc ''`

  - In a module: shows all available packages in current module, including dependencies and moonbitlang/core
  - In a package: shows all symbols in current package
  - Outside package: shows all available packages

- **Function/value lookup**: `moon doc "[@pkg.]value_or_function_name"`
  
- **Type lookup**: `moon doc "[@pkg.]Type_name"` (builtin type does not need package prefix)

- **Method/field lookup**: `moon doc "[@pkg.]Type_name::method_or_field_name"`

- **Package exploration**: `moon doc "@pkg"`
  - Show package `pkg` and list all its exported symbols
  - Example: `moon doc "@json"` - explore entire `@json` package
  - Example: `moon doc "@encoding/utf8"` - explore nested package

- **Globbing**: Use `*` wildcard for partial matches, e.g. `moon doc "String::*rev*"` to find all String methods with "rev" in their name

### `moon doc` Examples

````bash
# search for String methods in standard library:
$ moon doc "String"

type String

  pub fn String::add(String, String) -> String
  # ... more methods omitted ...

$ moon doc "@buffer" # list all symbols in  package buffer:
moonbitlang/core/buffer

fn from_array(ArrayView[Byte]) -> Buffer
# ... omitted ...

$ moon doc "@buffer.new" # list the specific function in a package:
package "moonbitlang/core/buffer"

pub fn new(size_hint? : Int) -> Buffer
  Creates ... omitted ...


$ moon doc "String::*rev*"  # globbing
package "moonbitlang/core/string"

pub fn String::rev(String) -> String
  Returns ... omitted ...
  # ... more

pub fn String::rev_find(String, StringView) -> Int?
  Returns ... omitted ...
````
**Best practice**: When implementing a feature, start with `moon doc` queries to discover available APIs before writing code. This is faster and more accurate than searching through files.

## `moon ide [peek-def|outline|find-references]` for code navigation and refactoring

For project-local symbols and navigation, use `moon ide outline .` to scan a package, `moon ide find-references <symbol>` to locate usages, and `moon ide peek-def` for inline definition context and locate toplevel symbols. 

These tools save tokens and more precise than grepping(grep display results in both definition and call site including comments too).

### `moon ide peek-def sym [-loc filename:line:col]` example

When the user ask: Can you check if `Parser::read_u32_leb128` is implemented correctly?

In this case, You can run `moon ide peek-def Parser::read_u32_leb128` to get the definition context: (this is better than `grep` since it searches the whole project by semantics)

``` file src/parse.mbt
L45:|///|
L46:|fn Parser::read_u32_leb128(self : Parser) -> UInt raise ParseError {
L47:|  ...
...:| }
```
Now you want to see the definition of `Parser` struct, you can run:

```bash
$ moon ide peek-def Parser -loc src/parse.mbt:46:4
Definition found at file src/parse.mbt
  | ///|
2 | priv struct Parser {
  |             ^^^^^^
  |   bytes : Bytes
  |   mut pos : Int
  | }  
  | 
```
For the `-loc` argument, the line number must be precise; the column can be approximate since 
the positonal argument `Parser` helps locate the position.

If the sym is toplevel symbol, the location can be omitted:
````bash
$ moon ide peek-def String::rev
Found 1 symbols matching 'String::rev':

`pub fn String::rev` in package moonbitlang/core/builtin at /Users/usrname/.moon/lib/core/builtin/string_methods.mbt:1039-1044
1039 | ///|
     | /// Returns a new string with the characters in reverse order. It respects
     | /// Unicode characters and surrogate pairs but not grapheme clusters.
     | pub fn String::rev(self : String) -> String {
     |   self[:].rev()
     | }         
````

### `moon ide outline [dir|file]` and `moon ide find-references <sym>` for Package Symbols

Use this to scan a package or file for top-level symbols and locate usages without grepping

- `moon ide outline dir` outlines the current package directory (per-file headers)
- `moon ide outline parser.mbt` outlines a single file
- Useful when you need a quick inventory of a package, or to find the right file before `goto-definition`
- `moon ide find-references TranslationUnit` finds all references to a symbol in the current module

```bash
$ moon ide outline .
spec.mbt:
 L003 | pub(all) enum CStandard {
        ...
 L013 | pub(all) struct Position {
        ...
```

```bash
$ moon ide find-references TranslationUnit
```

## Package Management

### Adding Dependencies

```sh
moon add moonbitlang/x        # Add latest version
moon add moonbitlang/x@0.4.6  # Add specific version
```

### Updating Dependencies

```sh
moon update                   # Update package index
```

### Typical Module configurations (`moon.mod.json`)

```json
{
  "name": "username/hello", // Required format for published modules
  "version": "0.1.0",
  "source": ".", // Source directory(optional, default: ".")
  "repository": "", // Git repository URL
  "keywords": [], // Search keywords
  "description": "...", // Module description
  "deps": {
    // Dependencies from mooncakes.io, using`moon add` to add dependencies
    "moonbitlang/x": "0.4.6"
  }
}
```

### Typical Package configuration (`moon.pkg.json`)

```json
{
  "is_main": true,                 // Creates executable when true
  "import": [                      // Package dependencies
    "username/hello/liba",         // Simple import, use @liba.foo() to call functions
    {
      "path": "moonbitlang/x/encoding",
      "alias": "libb"              // Custom alias, use @libb.encode() to call functions
    }
  ],
  "test-import": [...],            // Imports for black-box tests, similar to import
  "wbtest-import": [...]           // Imports for white-box tests, similar to import (rarely used)
}
```

Packages per directory, packages without `moon.pkg.json` are not recognized.

### Package Importing (used in moon.pkg.json)

- **Import format**: `"module_name/package_path"`
- **Usage**: `@alias.function()` to call imported functions
- **Default alias**: Last part of path (e.g., `liba` for `username/hello/liba`)
- **Package reference**: Use `@packagename` in test files to reference the
  tested package

**Package Alias Rules**:

- Import `"username/hello/liba"` → use `@liba.function()` (default alias is last path segment)
- Import with custom alias `{"path": "moonbitlang/x/encoding", "alias": "enc"}` → use `@enc.function()`
- In `_test.mbt` or `_wbtest.mbt` files, the package being tested is auto-imported

Example:

```mbt
///|
/// In main.mbt after importing "username/hello/liba" in `moon.pkg.json`
fn main {
  println(@liba.hello()) // Calls hello() from liba package
}
```

### Using Standard Library (moonbitlang/core)

**MoonBit standard library (moonbitlang/core) packages are automatically imported** - DO NOT add them to dependencies:

- ❌ **DO NOT** use `moon add` to add standard library packages like `moonbitlang/core/strconv`
- ❌ **DO NOT** add standard library packages to `"deps"` field of `moon.mod.json`
- ❌ **DO NOT** add standard library packages to `"import"` field of `moon.pkg.json`
- ✅ **DO** use them directly: `@strconv.parse_int()`, `@list.List`, `@array.fold()`, etc.

If you get an error like "cannot import `moonbitlang/core/strconv`", remove it from imports - it's automatically available.

### Creating Packages

To add a new package `fib` under `.`:

1. Create directory: `./fib/`
2. Add `./fib/moon.pkg.json`: `{}` -- Minimal valid moon.pkg.json
3. Add `.mbt` files with your code
4. Import in dependent packages:

   ```json
   {
     "import": [
        "username/hello/fib",
        ...
     ]
   }
   ```
For more advanced topics like `conditional compilation`, `link configuration`, `warning control`, and `pre-build commands`, see `references/advanced-moonbit-build.md`.

# MoonBit Language Tour

## Core facts

- **Expression‑oriented**: `if`, `match`, loops return values; last expression is the return.
- **References by default**: Arrays/Maps/structs mutate via reference; use `Ref[T]` for primitive mutability.
- **Blocks**: Separate top‑level items with `///|`. Generate code block‑by‑block.
- **Visibility**: `fn` private by default; `pub` exposes read/construct as allowed; `pub(all)` allows external construction.
- **Naming convention**: lower_snake for values/functions; UpperCamel for types/enums; enum variants start UpperCamel.
- **Packages**: No `import` in code files; call via `@alias.fn`. Configure imports in `moon.pkg.json`.
- **Placeholders**: `...` is a valid placeholder in MoonBit code for incomplete implementations.
- **Global values**: immutable by default and generally require type annotations.
- **Garbage collection**: MoonBit has a GC, there is no lifetime annotation, there's no ownership system.
  Unlike Rust, like F#, `let mut` is only needed when you want to reassign a variable, not for mutating fields of a struct or elements of an array/map.
- **Delimit top-level items with `///|` comments** so tools can split the file reliably.

## MoonBit Error Handling (Checked Errors)

MoonBit uses checked error-throwing functions, not unchecked exceptions. All errors are subtype of `Error`, we can declare our own error types by `suberror`.
Use `raise` in signatures to declare error types and let errors propagate by
default. Use `try?` to convert to `Result[...]` in tests, or `try { } catch { }`
to handle errors explicitly.

```mbt check
///|
/// Declare error types with 'suberror'
suberror ValueError String

///|
/// Tuple struct to hold position info
struct Position(Int, Int) derive(ToJson, Show, Eq) 

///|
/// ParseError is subtype of Error
pub(all) suberror ParseError {
  InvalidChar(pos~:Position, Char) // pos is labeled
  InvalidEof(pos~:Position)
  InvalidNumber(pos~:Position, String)
  InvalidIdentEscape(pos~:Position)
} derive(Eq, ToJson, Show)

///|
/// Functions declare what they can throw
fn parse_int(s : String, position~ : Position) -> Int raise ParseError {
  // 'raise' throws an error
  if s is "" {
    raise ParseError::InvalidEof(pos=position)
  }
  ... // parsing logic
}

///|
/// Just declare `raise` to not track specific error types
fn div(x : Int, y : Int) -> Int raise {
  if y is 0 {
    fail("Division by zero")
  }
  x / y
}

///|
test "inspect raise function" {
  let result : Result[Int, Error] = try? div(1, 0) 
  guard result is Err(Failure(msg)) && msg.contains("Division by zero") else {
    fail("Expected error")
  }  
}

// Three ways to handle errors:

///|
/// Propagate automatically
fn use_parse(position~: Position) -> Int raise ParseError {
  let x = parse_int("123", position=position)
  // Error auto-propagates by default.
  // Unlike Swift, you do not need to mark `try` for functions that can raise
  // errors; the compiler infers it automatically. This keeps error handling
  // explicit but concise.
  x * 2
}

///|
/// Mark `raise` for all possible errors, do not care which error it is.
/// For quick prototypes, `raise` is acceptable.
fn use_parse2(position~: Position) -> Int raise {
  let x = parse_int("123", position=position)
  x * 2
}

///|
/// Convert to Result with try?
fn safe_parse(s : String, position~: Position) -> Result[Int, ParseError] {
  let val1 : Result[_] = try? parse_int(s, position=position) // Returns Result[Int, ParseError]
  // try! is rarely used - it panics on error, similar to unwrap() in Rust
  // let val2 : Int = try! parse_int(s) // Returns Int otherwise crash

  // Alternative explicit handling:
  let val3 = try parse_int(s, position=position) catch {
    err => Err(err)
  } noraise { // noraise block is optional - handles the success case
    v => Ok(v)
  }
  ...
}

///|
/// Handle with try-catch
fn handle_parse(s : String, position~: Position) -> Int {
  try parse_int(s, position=position) catch {
    ParseError::InvalidEof => {
      println("Parse failed: InvalidEof")
      -1 // Default value
    }
    _ => 2
  }
}
```

Important: When calling a function that can raise errors, if you only want to
propagate the error, you do not need any marker; the compiler infers it.

## Integers, Char

MoonBit supports Byte, Int16, Int, UInt16, UInt, Int64, UInt64, etc. When the type is known,
the literal can be overloaded:

```mbt check
///|
test "integer and char literal overloading disambiguation via type in the current context" {
  let a0 = 1 // a is Int by default
  let (int, uint, uint16, int64, byte) : (Int, UInt, UInt16, Int64, Byte) = (
    1, 1, 1, 1, 1,
  )
  assert_eq(int, uint16.to_int())
  let a1 : Int = 'b' // this also works, a5 will be the unicode value
  let a2 : Char = 'b'
}
```
## Bytes (Immutable)

```mbt check
///|
test "bytes literals overloading and indexing" {
  let b0 : Bytes = b"abcd"
  let b1 : Bytes = "abcd" // b" prefix is optional, when we know the type
  let b2 : Bytes = [0xff, 0x00, 0x01] // Array literal overloading
  guard b0 is [b'a', ..] && b0[1] is b'b' else {
    // Bytes can be pattern matched as BytesView and indexed
    fail("unexpected bytes content")
  }
  
}
```
## Array (Resizable)

```mbt check
///|
test "array literals overloading: disambiguation via type in the current context" {
  let a0 : Array[Int] = [1, 2, 3] // resizable
  let a1 : FixedArray[Int] = [1, 2, 3] // Fixed size
  let a2 : ReadOnlyArray[Int] = [1, 2, 3] 
  let a3 : ArrayView[Int] = [1, 2, 3]
}
```
## String (Immutable UTF-16)
`s[i]` returns a code unit (UInt16), `s.get_char(i)` returns `Char?`.
Since MoonBit supports char literal overloading, you can write code snippets like this:

```mbt check
///|
test "string indexing and utf8 encode/decode" {
  let s = "hello world"
  let b0 : UInt16 = s[0]
  guard(b0 is ('\n' | 'h' | 'b' | 'a'..='z') && s is [.."hello", ..rest]) else {
    fail("unexpected string content")
  }
  guard rest is " world" // otherwise will crash (guard without else)
  
  // In check mode (expression with explicit type), ('\n' : UInt16) is valid.
  
  // Using get_char for Option handling
  let b1 : Char? = s.get_char(0)
  assert_true(b1 is Some('a'..='z'))

  // ⚠️ Important: Variables won't work with direct indexing
  let eq_char : Char = '='
  // s[0] == eq_char // ❌ Won't compile - eq_char is not a literal, lhs is UInt while rhs is Char
  // Use: s[0] == '=' or s.get_char(0) == Some(eq_char)
  let bytes = @encoding/utf8.encode("中文") // utf8 encode package is in stdlib
  assert_true(bytes is [0xe4, 0xb8, 0xad, 0xe6, 0x96, 0x87])
  let s2 : String = @encoding/utf8.decode(bytes) // decode utf8 bytes back to String
  assert_true(s2 is "中文")
  for c in "中文" {
    let _ : Char = c // unicode safe iteration  
    println("char: \{c}") // iterate over chars
  }
}
```

### String Interpolation && StringBuilder

MoonBit uses `\{}` for string interpolation, for custom types, it needs implement trait `Show`

```mbt check
///|
test "string interpolation basics" {

  let name : String = "Moon"
  let config = { "cache": 123 }
  let version = 1.0
  println("Hello \{name} v\{version}") // "Hello Moon v1.0"
  // ❌ Wrong - quotes inside interpolation not allowed:
  // println("  - Checking if 'cache' section exists: \{config["cache"]}")

  // ✅ Correct - extract to variable first:
  let has_key = config["cache"] // `"` not allowed in interpolation
  println("  - Checking if 'cache' section exists: \{has_key}")
  
  let sb = StringBuilder::new()
  sb..write_char('[') // dotdot for imperative method chaining
    ..write_view([1,2,3].map((x) => "\{x}").join(","))
    ..write_char(']')
  inspect(sb.to_string(), content="[1,2,3]")
}
```

Expressions inside `\{}` can only be _basic expressions_ (no quotes, newlines, or nested interpolations). String literals are not allowed as it makes lexing too difficult.


### Multiple line strings

```mbt check
///|
test "multi-line string literals" {  
  let multi_line_string : String =
    #|Hello "world"
    #|World
    #|
  let multi_line_string_with_interp : String =
    $|Line 1 ""
    $|Line 2 \{1+2}
    $|
  // no escape in `#|`, 
  // only escape '\{..}` in `$|`
  assert_eq(multi_line_string, "Hello \"world\"\nWorld\n")  
  assert_eq(multi_line_string_with_interp, "Line 1 \"\"\nLine 2 3\n")
}
```

## Map (Mutable, Insertion-Order Preserving)

```mbt check
///|
test "map literals and common operations" {
  // Map literal syntax
  let map : Map[String, Int] = { "a": 1, "b": 2, "c": 3 }  
  let empty : Map[String, Int] = {} // Empty map, preferred
  let also_empty : Map[String, Int] = Map::new()
  // From array of pairs
  let from_pairs : Map[String, Int] = Map::from_array([("x", 1), ("y", 2)])

  // Set/update value
  map["new-key"] = 3
  map["a"] = 10 // Updates existing key

  // Get value - returns Option[T]
  guard map is { "new-key": 3, "missing"? : None, .. } else {
    fail("unexpected map contents")
  }

  // Direct access (panics if key missing)
  let value : Int = map["a"] // value = 10

  // Iteration preserves insertion order
  for k, v in map {
    println("\{k}: \{v}") // Prints: a: 10, b: 2, c: 3, new-key: 3
  }

  // Other common operations
  map.remove("b")
  guard map is { "a": 10, "c": 3, "new-key": 3, .. } && map.length() == 3 else {
    // "b" is gone, only 3 elements left  
    fail("unexpected map contents after removal")
  }
}
```

## View Types

**Key Concept**: View types (`StringView`, `BytesView`, `ArrayView[T]`) are zero-copy, non-owning read-only slices created with the `[:]` syntax. They don't allocate memory and are ideal for passing sub-sequences without copying data, for function which takes String, Bytes, Array, they also take *View(implicit conversion).

- `String` → `StringView` via `s[:]` or `s[start:end]`
- `Bytes` → `BytesView` via `b[:]` or `b[start:end]`
- `Array[T]`, `FixedArray[T]`, `ReadOnlyArray[T] → `ArrayView[T]` via `a[:]` or `a[start:end]`

**Important**: StringView slice is slightly different due to unicode safety:
`s[a:b]` may raise an error at surrogate boundaries (UTF-16 encoding edge case). You have two options:

- Use `try! s[a:b]` if you're certain the boundaries are valid (crashes on invalid boundaries)
- Let the error propagate to the caller for proper handling

**When to use views**:

- Pattern matching with rest patterns (`[first, .. rest]`)
- Passing slices to functions without allocation overhead
- Avoiding unnecessary copies of large sequences

Convert back with `.to_string()`, `.to_bytes()`, or `.to_array()` when you need ownership. (`moon doc StringView`)

## User defined types(`enum`, `struct`)

```mbt check
///|
enum Tree[T] {
  Leaf(T) // Unlike Rust, no comma here
  Node(left~ : Tree[T], T, right~ : Tree[T]) // enum can use labels
} derive(Show, ToJson) // derive traits for Tree

///|
pub fn Tree::sum(tree : Tree[Int]) -> Int {
  match tree {
    Leaf(x) => x 
    // we don't need to write Tree::Leaf, when `tree` has a known type
    Node(left~, x, right~) => left.sum() + x + right.sum() // method invoked in dot notation
  }
}

///|
struct Point {
  x : Int
  y : Int
} derive(Show, ToJson) // derive traits for Point

test "user defined types: enum and struct" {
  @json.inspect(Point::{ x: 10, y: 20 }, content=({"x":10,"y":20}))
}

```

## Functional `for` loop


```mbt check
pub fn binary_search(
  arr : ArrayView[Int],
  value : Int,
) -> Result[Int, Int] {
  let len = arr.length()
  // functional for loop:
  // initial state ; [predicate] ; [post-update] {
  // loop body with `continue` to update state
  //} else { // exit block
  // }
  // predicate and post-update are optional
  for i = 0, j = len; i < j; {
    // post-update is omitted, we use `continue` to update state
    let h = i + (j - i) / 2
    if arr[h] < value {
      continue h + 1, j // functional update of loop state
    } else {
      continue i, h // functional update of loop state
    }
  } else { // exit of for loop
    if i < len && arr[i] == value {
      Ok(i)
    } else {
      Err(i)
    }
  } where {
    invariant : 0 <= i && i <= j && j <= len,
    invariant : i == 0 || arr[i - 1] < value,
    invariant : j == len || arr[j] >= value,
    reasoning :
      #|For a sorted array, the boundary invariants are witnesses:
      #|  - `arr[i-1] < value` implies all arr[0..i) < value (by sortedness)
      #|  - `arr[j] >= value` implies all arr[j..len) >= value (by sortedness)
      #|
      #|Preservation proof:
      #|  - When arr[h] < value: new_i = h+1, and arr[new_i - 1] = arr[h] < value ✓
      #|  - When arr[h] >= value: new_j = h, and arr[new_j] = arr[h] >= value ✓
      #|
      #|Termination: j - i decreases each iteration (h is strictly between i and j)
      #|
      #|Correctness at exit (i == j):
      #|  - By invariants: arr[0..i) < value and arr[i..len) >= value
      #|  - So if value exists, it can only be at index i
      #|  - If arr[i] != value, then value is absent and i is the insertion point
      #|
  }
}

///|
test "functional for loop control flow" {
  let arr : Array[Int] = [1, 3, 5, 7, 9]
  inspect(binary_search(arr,5), content="Ok(2)") // Array to ArrayView implicit conversion when passing as arguments
  inspect(binary_search(arr,6), content="Err(3)")
  // for iteration is supported too
  for i, v in arr {
    println("\{i}: \{v}") // `i` is index, `v` is value
  }
}
```
You are *STRONGLY ENCOURAGED* to use functional `for` loops instead of imperative loops
*WHENEVER POSSIBLE*, as they are easier to reason about.

### Loop Invariants with `where` Clause

The `where` clause attaches **machine-checkable invariants** and **human-readable reasoning** to functional `for` loops. This enables formal verification thinking while keeping the code executable. Note for trivial loops, you are encouraged to convert it into `for .. in` so no reasoning is needed.

**Syntax:**
```mbt nocheck
for ... {
  ...
} where {
  invariant : <boolean_expr>,   // checked at runtime in debug builds
  invariant : <boolean_expr>,   // multiple invariants allowed
  reasoning : <string>         // documentation for proof sketch
}
```

**Writing Good Invariants:**

1. **Make them checkable**: Invariants must be valid MoonBit boolean expressions using loop variables and captured values.

2. **Use boundary witnesses**: For properties over ranges (e.g., "all elements in arr[0..i) satisfy P"), check only boundary elements. For sorted arrays, `arr[i-1] < value` implies all `arr[0..i) < value`.

3. **Handle edge cases with `||`**: Use patterns like `i == 0 || arr[i-1] < value` to handle boundary conditions where the check would be out of bounds.

4. **Cover three aspects in reasoning**:
   - **Preservation**: Why each `continue` maintains the invariants
   - **Termination**: Why the loop eventually exits (e.g., a decreasing measure)
   - **Correctness**: Why the invariants at exit imply the desired postcondition

## Label and Optional Parameters

Good example: use labeled and optional parameters

```mbt check
///|
fn g(
  positional : Int,
  required~ : Int,
  optional? : Int, // no default => Option
  optional_with_default? : Int = 42, // default => plain Int
) -> String {
  // These are the inferred types inside the function body.
  let _ : Int = positional
  let _ : Int = required
  let _ : Int? = optional
  let _ : Int = optional_with_default
  "\{positional},\{required},\{optional},\{optional_with_default}"
}

///|
test {
  inspect(g(1, required=2), content="1,2,None,42")
  inspect(g(1, required=2, optional=3), content="1,2,Some(3),42")
  inspect(g(1, required=4, optional_with_default=100), content="1,4,None,100")
}
```

Misuse: `arg : Type?` is not an optional parameter.
Callers still must pass it (as `None`/`Some(...)`).

```mbt check
///|
fn with_config(a : Int?, b : Int?, c : Int) -> String {
  "\{a},\{b},\{c}"
}

///|
test {
  inspect(with_config(None, None, 1), content="None,None,1")
  inspect(with_config(Some(5), Some(5), 1), content="Some(5),Some(5),1")
}
```

Anti-pattern: `arg? : Type?` (no default => double Option).
If you want a defaulted optional parameter, write `b? : Int = 1`, not `b? : Int? = Some(1)`.

```mbt check
///|
fn f_misuse(a? : Int?, b? : Int = 1) -> Unit {
  let _ : Int?? = a // rarely intended
  let _ : Int = b

}
// How to fix: declare `(a? : Int, b? : Int = 1)` directly.

///|
fn f_correct(a? : Int, b? : Int = 1) -> Unit {
  let _ : Int? = a
  let _ : Int = b

}

///|
test {
  f_misuse(b=3)
  f_misuse(a=Some(5), b=2) // works but confusing
  f_correct(b=2)
  f_correct(a=5)
}
```

Bad example: `arg : APIOptions` (use labeled optional parameters instead)

```mbt check
///|
/// Do not use struct to group options.
struct APIOptions {
  width : Int?
  height : Int?
}

///|
fn not_idiomatic(opts : APIOptions, arg : Int) -> Unit {

}

///|
test {
  // Hard to use in call site
  not_idiomatic({ width : Some(5), height : None }, 10)
  not_idiomatic({ width : None, height : None }, 10)
}
```

## More details

For deeper syntax, types, and examples, read `references/moonbit-language-fundamentals.mbt.md`.
