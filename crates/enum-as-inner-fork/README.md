Forked to add #[inline]

# enum-as-inner

A deriving proc-macro for generating functions to automatically give access to the inner members of enum.

## Basic unnamed field case

The basic case is meant for single item enums, like:

```rust
use enum_as_inner::EnumAsInner;

#[derive(Debug, EnumAsInner)]
enum OneEnum {
    One(u32),
}
```

where the inner item can be retrieved with the `as_*()`/`as_*_mut()` or with the `into_*()` functions:

```rust
let one = OneEnum::One(1);

assert_eq!(*one.as_one().unwrap(), 1);
assert_eq!(one.into_one().unwrap(), 1);

let mut one = OneEnum::One(2);

assert_eq!(*one.as_one().unwrap(), 1);
assert_eq!(*one.as_one_mut().unwrap(), 1);
assert_eq!(one.into_one().unwrap(), 1);
```

where the result is either a reference for inner items or a tuple containing the inner items.

## Unit case

This will return true if enum's variant matches the expected type

```rust
use enum_as_inner::EnumAsInner;

#[derive(EnumAsInner)]
enum UnitVariants {
    Zero,
    One,
    Two,
}

let unit = UnitVariants::Two;

assert!(unit.is_two());
```

## Mutliple, unnamed field case

This will return a tuple of the inner types:

```rust
use enum_as_inner::EnumAsInner;

#[derive(Debug, EnumAsInner)]
enum ManyVariants {
    One(u32),
    Two(u32, i32),
    Three(bool, u32, i64),
}
```

And can be accessed like:

```rust
let mut many = ManyVariants::Three(true, 1, 2);

assert_eq!(many.as_three().unwrap(), (&true, &1_u32, &2_i64));
assert_eq!(many.as_three_mut().unwrap(), (&mut true, &mut 1_u32, &mut 2_i64));
assert_eq!(many.into_three().unwrap(), (true, 1_u32, 2_i64));
```

## Multiple, named field case

This will return a tuple of the inner types, like the unnamed option:

```rust
use enum_as_inner::EnumAsInner;

#[derive(Debug, EnumAsInner)]
enum ManyVariants {
    One{ one: u32 },
    Two{ one: u32, two: i32 },
    Three{ one: bool, two: u32, three: i64 },
}
```

And can be accessed like:

```rust
let mut many = ManyVariants::Three{ one: true, two: 1, three: 2 };

assert_eq!(many.as_three().unwrap(), (&true, &1_u32, &2_i64));
assert_eq!(many.as_three_mut().unwrap(), (&mut true, &mut 1_u32, &mut 2_i64));
assert_eq!(many.into_three().unwrap(), (true, 1_u32, 2_i64));
```
