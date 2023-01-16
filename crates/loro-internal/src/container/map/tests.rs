#![cfg(test)]

use std::collections::HashMap;
use std::sync::Arc;

use proptest::prelude::*;
use proptest::proptest;

use crate::{fx_map, LoroCore, LoroValue};

#[test]
fn basic() {
    let mut loro = LoroCore::default();
    let _weak = Arc::downgrade(&loro.log_store);
    let mut container = loro.get_map("map");
    container.insert(&loro, "haha", LoroValue::I32(1)).unwrap();
    let ans = fx_map!(
        "haha".into() => LoroValue::I32(1)
    );

    assert_eq!(container.get_value(), LoroValue::Map(Box::new(ans)));
}

mod map_proptest {
    use crate::{tests::PROPTEST_FACTOR_10, value::proptest::gen_insert_value};

    use super::*;

    proptest! {
        #[test]
        fn insert(
            key in prop::collection::vec("[a-z]", 0..10 * PROPTEST_FACTOR_10),
            value in prop::collection::vec(gen_insert_value(), 0..10 * PROPTEST_FACTOR_10)
        ) {
            let mut loro = LoroCore::default();
            let _weak = Arc::downgrade(&loro.log_store);
            let mut container = loro.get_map("map");
            let mut map: HashMap<String, LoroValue> = HashMap::new();
            for (k, v) in key.iter().zip(value.iter()) {
                map.insert(k.clone(), v.clone());
                container.insert(&loro, k.as_str(), v.clone()).unwrap();
                let snapshot = container.get_value();
                for (key, value) in snapshot.as_map().unwrap().iter() {
                    assert_eq!(map.get(&key.to_string()).cloned(), Some(value.clone()));
                }
            }
        }
    }
}
