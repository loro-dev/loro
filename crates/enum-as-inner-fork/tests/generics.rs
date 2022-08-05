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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, EnumAsInner)]
enum WithGenerics<T: Clone + Copy> {
    A(T),
    B(T),
}

#[test]
fn with_generics() {
    let mut with_generics = WithGenerics::A(100);

    assert!(with_generics.as_a().is_some());
    assert!(with_generics.as_b().is_none());

    assert_eq!(with_generics.into_a().unwrap(), 100);
    assert_eq!(*with_generics.as_a().unwrap(), 100);
    assert_eq!(*with_generics.as_a_mut().unwrap(), 100);

    assert!(with_generics.into_b().is_err());
    assert!(with_generics.as_b().is_none());
    assert!(with_generics.as_b_mut().is_none());
}
