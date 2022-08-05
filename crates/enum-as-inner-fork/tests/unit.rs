#![warn(
    clippy::default_trait_access,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::unimplemented,
    clippy::use_self,
    missing_copy_implementations,
    missing_docs,
    non_snake_case,
    non_upper_case_globals,
    rust_2018_idioms,
    unreachable_pub
)]

use enum_as_inner::EnumAsInner;

#[derive(EnumAsInner)]
enum UnitVariants {
    Zero,
    One,
    Two,
}

#[test]
fn test_zero_unit() {
    let unit = UnitVariants::Zero;

    assert!(unit.is_zero());
    assert!(!unit.is_one());
    assert!(!unit.is_two());
}

#[test]
fn test_one_unit() {
    let unit = UnitVariants::One;

    assert!(!unit.is_zero());
    assert!(unit.is_one());
    assert!(!unit.is_two());
}

#[test]
fn test_two_unit() {
    let unit = UnitVariants::Two;

    assert!(!unit.is_zero());
    assert!(!unit.is_one());
    assert!(unit.is_two());
}
