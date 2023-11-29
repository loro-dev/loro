#![cfg(test)]

use crate::{op::ListSlice, LoroValue};

#[cfg(proptest)]
pub const PROPTEST_FACTOR_10: usize = 10;
#[cfg(not(proptest))]
pub const PROPTEST_FACTOR_10: usize = 1;

#[cfg(proptest)]
pub const PROPTEST_FACTOR_1: usize = 1;
#[cfg(not(proptest))]
pub const PROPTEST_FACTOR_1: usize = 0;

#[test]
fn size_of() {
    use crate::change::Change;
    use crate::{
        container::{map::MapSet, ContainerID},
        id::ID,
        op::{Op, RawOpContent},
        span::IdSpan,
        InternalString,
    };

    println!("Change {}", std::mem::size_of::<Change>());
    println!("Op {}", std::mem::size_of::<Op>());
    println!("InsertContent {}", std::mem::size_of::<RawOpContent>());
    println!("MapSet {}", std::mem::size_of::<MapSet>());
    println!("ListSlice {}", std::mem::size_of::<ListSlice>());
    println!("LoroValue {}", std::mem::size_of::<LoroValue>());
    println!("ID {}", std::mem::size_of::<ID>());
    println!("Vec {}", std::mem::size_of::<Vec<ID>>());
    println!("IdSpan {}", std::mem::size_of::<IdSpan>());
    println!("ContainerID {}", std::mem::size_of::<ContainerID>());
    println!("InternalString {}", std::mem::size_of::<InternalString>());
}
