#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

mod import_atomicity;
mod op_count;
mod replay_base;
mod text_arena_growth;

use crate::{op::ListSlice, LoroValue};

/// Two-way sync of two docs.
pub(crate) fn exchange(a: &crate::LoroDoc, b: &crate::LoroDoc) {
    let ua = a
        .export(crate::loro::ExportMode::updates(&b.oplog_vv()))
        .unwrap();
    let ub = b
        .export(crate::loro::ExportMode::updates(&a.oplog_vv()))
        .unwrap();
    a.import(&ub).unwrap();
    b.import(&ua).unwrap();
}

pub const PROPTEST_FACTOR_10: usize = 1;
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
mod undo_atomicity;
