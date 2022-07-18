#![cfg(test)]

use std::pin::Pin;

use fxhash::FxHashMap;

use crate::{
    configure::Configure,
    container::{Container, ContainerType},
    fx_map,
    value::InsertValue,
    LoroCore, LoroValue,
};

use super::*;

#[test]
fn basic() {
    let mut loro = LoroCore::default();
    let mut container = loro.get_map_container("map".into());
    container.insert("haha".into(), InsertValue::Int32(1));
    let ans = fx_map!(
        "haha".into() => LoroValue::Integer(1)
    );

    dbg!(container.snapshot().value());
    dbg!(&ans);
    assert_eq!(*container.snapshot().value(), LoroValue::Map(ans));
}
