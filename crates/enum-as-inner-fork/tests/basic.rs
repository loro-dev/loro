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

#[derive(Debug, EnumAsInner)]
enum EmptyTest {}

#[test]
fn test_empty() {
    let empty = None::<EmptyTest>;

    assert!(empty.is_none());
}

#[derive(Debug, EnumAsInner)]
enum EmptyParendsTest {
    Empty(),
}

#[test]
fn test_empty_parends() {
    let mut empty = EmptyParendsTest::Empty();

    empty
        .as_empty()
        .expect("should have been something and a unit");
    empty
        .as_empty_mut()
        .expect("should have been something and a unit");

    empty
        .into_empty()
        .expect("should have been something and a unit");
}

#[derive(Debug, EnumAsInner)]
enum OneTest {
    One(u32),
}

#[test]
fn test_one() {
    let mut empty = OneTest::One(1);

    assert_eq!(*empty.as_one().unwrap(), 1);
    assert_eq!(*empty.as_one_mut().unwrap(), 1);
    assert_eq!(empty.into_one().unwrap(), 1);
}

#[derive(Debug, EnumAsInner)]
enum MultiTest {
    Multi(u32, u32),
}

#[test]
fn test_multi() {
    let mut multi = MultiTest::Multi(1, 1);

    assert_eq!(multi.as_multi().unwrap(), (&1_u32, &1_u32));
    assert_eq!(multi.as_multi_mut().unwrap(), (&mut 1_u32, &mut 1_u32));
    assert_eq!(multi.into_multi().unwrap(), (1_u32, 1_u32));
}
