#![cfg(test)]

use std::collections::HashMap;
use std::sync::Arc;

use fxhash::FxHashMap;
use proptest::prelude::*;
use proptest::proptest;

use crate::value::proptest::gen_insert_value;
use crate::Container;

use crate::{fx_map, value::InsertValue, LoroCore, LoroValue};

#[test]
fn basic() {
    let mut loro = LoroCore::default();
    let _weak = Arc::downgrade(&loro.log_store);
    let mut container = loro.get_or_create_root_map("map").unwrap();
    container.insert("haha".into(), InsertValue::Int32(1));
    let ans = fx_map!(
        "haha".into() => LoroValue::Integer(1)
    );

    assert_eq!(container.get_value(), LoroValue::Map(ans));
}

mod map_proptest {
    use crate::tests::PROPTEST_FACTOR_10;

    use super::*;

    proptest! {
        #[test]
        fn insert(
            key in prop::collection::vec("[a-z]", 0..10 * PROPTEST_FACTOR_10),
            value in prop::collection::vec(gen_insert_value(), 0..10 * PROPTEST_FACTOR_10)
        ) {
            let mut loro = LoroCore::default();
            let _weak = Arc::downgrade(&loro.log_store);
            let mut container = loro.get_or_create_root_map("map").unwrap();
            let mut map: HashMap<String, InsertValue> = HashMap::new();
            for (k, v) in key.iter().zip(value.iter()) {
                map.insert(k.clone(), v.clone());
                container.insert(k.clone().into(), v.clone());
                let snapshot = container.get_value();
                for (key, value) in snapshot.as_map().unwrap().iter() {
                    assert_eq!(map.get(&key.to_string()).map(|x|x.clone().into()), Some(value.clone()));
                }
            }
        }
    }
}
