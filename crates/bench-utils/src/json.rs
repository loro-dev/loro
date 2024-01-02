use std::sync::Arc;

use arbitrary::{Arbitrary, Unstructured};
pub use loro_common::LoroValue;

use crate::ActionTrait;

#[derive(Arbitrary, Debug, PartialEq, Eq, Clone)]
pub enum JsonAction {
    InsertMap {
        key: String,
        value: LoroValue,
    },
    InsertList {
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=1024))]
        index: usize,
        value: LoroValue,
    },
    DeleteList {
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=1024))]
        index: usize,
    },
    InsertText {
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=1024))]
        index: usize,
        s: String,
    },
    DeleteText {
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=1024))]
        index: usize,
        #[arbitrary(with = |u: &mut Unstructured| u.int_in_range(0..=128))]
        len: usize,
    },
}

const MAX_LEN: usize = 1000;
impl ActionTrait for JsonAction {
    fn normalize(&mut self) {
        match self {
            JsonAction::InsertMap { key: _, value } => {
                normalize_value(value);
            }

            JsonAction::InsertList { index: _, value } => {
                normalize_value(value);
            }
            JsonAction::DeleteList { index } => {
                *index %= MAX_LEN;
            }
            JsonAction::InsertText { .. } => {}
            JsonAction::DeleteText { .. } => {}
        }
    }
}

fn normalize_value(value: &mut LoroValue) {
    match value {
        LoroValue::Double(f) => {
            if f.is_nan() {
                *f = 0.0;
            }
        }
        LoroValue::List(l) => {
            for v in Arc::make_mut(l).iter_mut() {
                normalize_value(v);
            }
        }
        LoroValue::Map(m) => {
            for (_, v) in Arc::make_mut(m).iter_mut() {
                normalize_value(v);
            }
        }
        LoroValue::Container(_) => {
            *value = LoroValue::Null;
        }
        _ => {}
    }
}
