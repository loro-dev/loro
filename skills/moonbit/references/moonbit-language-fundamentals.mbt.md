# MoonBit Language Fundamentals


## Quick reference:

```mbt check
///|
/// comments doc string
pub fn sum(x : Int, y : Int) -> Int {
  x + y
}

///|
struct Rect {
  width : Int
  height : Int
}

///|
fn Rect::area(self : Rect) -> Int {
  self.width * self.height
}

///|
pub impl Show for Rect with output(_self, logger) {
  logger.write_string("Rect")
}

///|
enum MyOption {
  MyNone
  MySome(Int)
} derive(Show, ToJson, Eq, Compare)

///|
///  match + loops are expressions
test "everything is expression in MoonBit" {
  // tuple
  let (n, opt) = (1, MySome(2))
  // if expressions return values
  let msg : String = if n > 0 { "pos" } else { "non-pos" }
  let res = match opt {
    MySome(x) => {
      inspect(x, content="2")
      1
    }
    MyNone => 0
  }
  let status : Result[Int, String] = Ok(10)
  // match expressions return values
  let description = match status {
    Ok(value) => "Success: \{value}"
    Err(error) => "Error: \{error}"
  }
  let array = [1, 2, 3, 4, 5]
  let mut i = 0 // mutable bindings (local only, globals are immutable)
  let target = 3
  // loops return values with 'break'
  let found : Int? = while i < array.length() {
    if array[i] == target {
      break Some(i) // Exit with value
    }
    i = i + 1
  } else { // Value when loop completes normally
    None
  }
  assert_eq(found, Some(2)) // Found at index 2
}

///|
/// global bindings
pub let my_name : String = "MoonBit"

///|
pub const PI : Double = 3.14159 // constants use UPPER_SNAKE or PascalCase

///|
pub fn maximum(xs : Array[Int]) -> Int raise {
  // Toplevel functions are *mutually recursive* by default
  // `raise` annotation means the function would raise any Error
  //  Only add `raise XXError` when you do need track the specific error type
  match xs {
    [] => fail("Empty array") // fail() is built-in for generic errors
    [x] => x
    // pattern match over array, the `.. rest` is a rest pattern
    // it is of type `ArrayView[Int]` which is a slice
    [x, .. rest] => {
      let mut max_val = x // `mut` only allowed in local bindings
      for y in rest {
        if y > max_val {
          max_val = y
        }
      }
      max_val // return can be omitted if the last expression is the return value
    }
  }
}

///|
/// pub(all) means it can be both read and created outside the package
pub(all) struct Point {
  x : Int
  mut y : Int
} derive(Show, ToJson)

///|
pub enum MyResult[T, E] {
  MyOk(T) // semicolon `;` is optional when we have a newline
  MyErr(E) // Enum variants must start uppercase
} derive(Show, Eq, ToJson)
// pub means it can only be pattern matched outside the package
// but it can not be created outside the package, use `pub(all)` otherwise

///|
/// pub (open) means the trait can be implemented for outside packages
pub(open) trait Comparable {
  compare(Self, Self) -> Int // `Self` refers to the implementing type
}

///|
test "inspect test" {
  let result = sum(1, 2)
  inspect(result, content="3")
  // The `content` can be auto-corrected by running `moon test --update`
  let point = Point::{ x: 10, y: 20 }
  // For complex structures, use @json.inspect for better readability:
  @json.inspect(point, content={ "x": 10, "y": 20 })
}
```


## Complex Types

```mbt check
///|
pub type UserId = Int // Int is aliased to UserId - like symlink

///|
///  Tuple-struct for callback
pub struct Handler((String) -> Unit) // A newtype wrapper

///|
/// Tuple-struct syntax for single-field newtypes
struct Meters(Int) // Tuple-struct syntax

///|
let distance : Meters = Meters(100)

///|
let raw : Int = distance.0 // Access first field with .0

///|
struct Addr {
  host : String
  port : Int
} derive(Show, Eq, ToJson, FromJson)

///|
/// Structural types with literal syntax
let config : Addr = Addr::{
  // `Type::` can be omitted since the type is already known
  host: "localhost",
  port: 8080,
}


```

## Common Derivable Traits

Most types can automatically derive standard traits using the `derive(...)` syntax:

- **`Show`** - Enables `to_string()` and string interpolation with `\{value}`
- **`Eq`** - Enables `==` and `!=` equality operators
- **`Compare`** - Enables `<`, `>`, `<=`, `>=` comparison operators
- **`ToJson`** - Enables `@json.inspect()` for readable test output
- **`Hash`** - Enables use as Map keys

```mbt check
///|
struct Coordinate {
  x : Int
  y : Int
} derive(Show, Eq, ToJson)

///|
enum Status {
  Active
  Inactive
} derive(Show, Eq, Compare)
```

**Best practice**: Always derive `Show` and `Eq` for data types. Add `ToJson` if you plan to test them with `@json.inspect()`.

## Reference Semantics by Default

MoonBit passes most types by reference semantically (the optimizer may copy
immutables):

```mbt check
///|
///  Structs with 'mut' fields are always passed by reference
struct Counter {
  mut value : Int
}

///|
fn increment(c : Counter) -> Unit {
  c.value += 1 // Modifies the original
}

///|
/// Arrays and Maps are mutable references
fn modify_array(arr : Array[Int]) -> Unit {
  arr[0] = 999 // Modifies original array
}

///|
test "reference semantics" {
  let counter : Ref[Int] = Ref::{ val: 0 }
  counter.val += 1
  assert_true(counter.val is 1)
  let arr : Array[Int] = [1, 2, 3] // unlike Rust, no `mut` keyword needed
  modify_array(arr)
  assert_true(arr[0] is 999)
  let mut x = 3 // `mut` neeed for re-assignment to the bindings
  x += 2
  assert_true(x is 5)
}
```

## Pattern Matching

```mbt check
///|
#warnings("-unused_value")
test "pattern match over Array, struct and StringView" {
  let arr : Array[Int] = [10, 20, 25, 30]
  match arr {
    [] => ... // empty array
    [single] => ... // single element
    [first, .. middle, rest] => {
      let _ : ArrayView[Int] = middle // middle is ArrayView[Int]  
      assert_true(first is 10 && middle is [20, 25] && rest is 30)
    }
  }
  fn process_point(point : Point) -> Unit {
    match point {
      { x: 0, y: 0 } => ...
      { x, y } if x == y => ...
      { x, .. } if x < 0 => ...
      ...
    }
  }
  /// StringView pattern matching for parsing
  fn is_palindrome(s : StringView) -> Bool {
    loop s {
      [] | [_] => true
      [a, .. rest, b] if a == b => continue rest
      // a is of type Char, rest is of type StringView
      _ => false
    }
  }


}
```

## Functional `loop` control flow

The `loop` construct is unique to MoonBit:

```mbt check
///|
/// Functional loop with pattern matching on loop variables
/// @list.List is from the standard library
fn sum_list(list : @list.List[Int]) -> Int {
  loop (list, 0) {
    (Empty, acc) => acc // Base case returns accumulator
    (More(x, tail=rest), acc) => continue (rest, x + acc) // Recurse with new values
  }
}

///|
///  Multiple loop variables with complex control flow
fn find_pair(arr : Array[Int], target : Int) -> (Int, Int)? {
  loop (0, arr.length() - 1) {
    (i, j) if i >= j => None
    (i, j) => {
      let sum = arr[i] + arr[j]
      if sum == target {
        Some((i, j)) // Found pair
      } else if sum < target {
        continue (i + 1, j) // Move left pointer
      } else {
        continue (i, j - 1) // Move right pointer
      }
    }
  }
}
```

**Note**: You must provide a payload to `loop`. If you want an infinite loop, use `while true { ... }` instead. The syntax `loop { ... }` without arguments is invalid.


## Methods and Traits

Methods use `Type::method_name` syntax, traits require explicit implementation:

```mbt check
///|
struct Rectangle {
  width : Double
  height : Double
}

///|
// Methods are prefixed with Type::
fn Rectangle::area(self : Rectangle) -> Double {
  self.width * self.height
}

///|
/// Static methods don't need self
fn Rectangle::new(w : Double, h : Double) -> Rectangle {
  { width: w, height: h }
}

///|
/// Show trait now uses output(self, logger) for custom formatting
/// to_string() is automatically derived from this
pub impl Show for Rectangle with output(self, logger) {
  logger.write_string("Rectangle(\{self.width}x\{self.height})")
}

///|
/// Traits can have non-object-safe methods
trait Named {
  name() -> String // No 'self' parameter - not object-safe
}

///|
/// Trait bounds in generics
fn[T : Show + Named] describe(value : T) -> String {
  "\{T::name()}: \{value.to_string()}"
}

///|
///  Trait implementation
impl Hash for Rectangle with hash_combine(self, hasher) {
  hasher..combine(self.width)..combine(self.height)
}
```

## Operator Overloading

MoonBit supports operator overloading through traits:

```mbt check
///|
struct Vector(Int, Int)

///|
/// Implement arithmetic operators
pub impl Add for Vector with add(self, other) {
  Vector(self.0 + other.0, self.1 + other.1)
}

///|
struct Person {
  age : Int
} derive(Eq)

///|
/// Comparison operators
pub impl Compare for Person with compare(self, other) {
  self.age.compare(other.age)
}

///|
test "overloading" {
  let v1 : Vector = Vector(1, 2)
  let v2 : Vector = Vector(3, 4)
  let _v3 : Vector = v1 + v2

}
```

## Access Control Modifiers

MoonBit has fine-grained visibility control:

```mbt check
///|
/// `fn` defaults to Private - only visible in current package
fn internal_helper() -> Unit {
  ...
}

///|
pub fn get_value() -> Int {
  ...
}

///|
// Struct (default) - type visible, implementation hidden
struct DataStructure {}

///|
/// `pub struct` defaults to readonly - can read, pattern match, but not create
pub struct Config {}

///|
///  Public all - full access
pub(all) struct Config2 {}

///|
/// Abstract trait (default) - cannot be implemented by
/// types outside this package
pub trait MyTrait {}

///|
///  Open for extension
pub(open) trait Extendable {}
```
