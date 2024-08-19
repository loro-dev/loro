use std::sync::Arc;

use fuzz::{
    actions::{
        ActionInner,
        ActionWrapper::{self, *},
        GenericAction,
    },
    container::{MapAction, TextAction, TextActionInner, TreeAction, TreeActionInner},
    crdt_fuzzer::{minify_error, test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::{ContainerType::*, LoroCounter, LoroDoc};

#[test]
fn empty() {
    test_multi_sites(5, vec![FuzzTarget::All], &mut []);
}

#[test]
fn one_op() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [Handle {
            site: 33,
            target: 158,
            container: 0,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 0,
                pos: 0,
                length: 0,
                prop: 0,
            }),
        }],
    );
}
