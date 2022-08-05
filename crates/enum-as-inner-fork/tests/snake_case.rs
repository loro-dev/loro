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
#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
enum MixedCaseVariants {
    XMLIsNotCool,
    Rust_IsCoolThough(u32),
    YMCA { named: i16 },
}

#[test]
fn test_xml_unit() {
    let mixed = MixedCaseVariants::XMLIsNotCool;

    assert!(mixed.is_xml_is_not_cool());
    assert!(mixed.as_rust_is_cool_though().is_none());
    assert!(mixed.as_ymca().is_none());
}

#[test]
fn test_rust_unnamed() {
    let mixed = MixedCaseVariants::Rust_IsCoolThough(42);

    assert!(!mixed.is_xml_is_not_cool());
    assert!(mixed.as_rust_is_cool_though().is_some());
    assert!(mixed.as_ymca().is_none());

    assert_eq!(*mixed.as_rust_is_cool_though().unwrap(), 42);
    assert_eq!(mixed.into_rust_is_cool_though().unwrap(), 42);
}

#[test]
fn test_ymca_named() {
    let mixed = MixedCaseVariants::YMCA { named: -32_768 };

    assert!(!mixed.is_xml_is_not_cool());
    assert!(mixed.as_rust_is_cool_though().is_none());
    assert!(mixed.as_ymca().is_some());

    assert_eq!(*mixed.as_ymca().unwrap(), (-32_768));
    assert_eq!(mixed.into_ymca().unwrap(), (-32_768));
}
