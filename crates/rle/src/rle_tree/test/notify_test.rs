use proptest::prop_compose;
use rand::{rngs::StdRng, SeedableRng};

use crate::{
    range_map::{RangeMap, WithStartEnd},
    rle_tree::tree_trait::CumulateTreeTrait,
    HasLength,
};

use super::super::*;
use std::ptr::NonNull;

type Value = WithStartEnd<usize, u64>;
type ValueTreeTrait = CumulateTreeTrait<Value, 4>;

#[derive(enum_as_inner::EnumAsInner, Debug)]
enum Interaction {
    Insert { from: usize, len: usize },
    Delete { from: usize, len: usize },
}

impl Interaction {
    fn apply<F, R>(&self, tree: &mut RleTree<Value, ValueTreeTrait>, rng: &mut R, notify: &mut F)
    where
        F: FnMut(&Value, *mut LeafNode<'_, Value, ValueTreeTrait>),
        R: rand::Rng,
    {
        match self {
            Interaction::Insert { from, len } => tree.with_tree_mut(|tree| {
                let mut from = *from;
                let len = *len;
                if tree.len() == 0 {
                    from = 0;
                } else {
                    from %= tree.len();
                }
                tree.insert_notify(
                    from,
                    WithStartEnd {
                        start: from,
                        end: from + len,
                        value: rng.next_u64(),
                    },
                    notify,
                );
            }),
            Interaction::Delete { from, len } => tree.with_tree_mut(|tree| {
                let mut from = *from;
                let mut len = *len;
                if tree.len() == 0 {
                    from = 0;
                } else {
                    from %= tree.len();
                }
                if from + len > tree.len() {
                    len = tree.len() - from;
                }
                tree.delete_range_notify(Some(from), Some(from + len), notify)
            }),
        }
    }
}

fn test(interactions: &[Interaction]) {
    let mut tree: RleTree<Value, ValueTreeTrait> = Default::default();
    let mut rng = StdRng::seed_from_u64(123);
    type ValueIndex<'a> = WithStartEnd<u64, NonNull<LeafNode<'a, Value, ValueTreeTrait>>>;
    let mut range_map: RangeMap<u64, ValueIndex<'_>> = Default::default();
    for interaction in interactions.iter() {
        let mut func = |value: &Value, node: *mut LeafNode<'_, Value, ValueTreeTrait>| {
            let ptr = unsafe { NonNull::new_unchecked(node as usize as *mut _) };
            range_map.set(
                value.value,
                WithStartEnd::new(value.value, value.value + value.len() as u64, ptr),
            );
            //println!("bbb {:#?}", value);
        };
        interaction.apply(&mut tree, &mut rng, &mut func);
        //println!("----------------------------------");
        tree.with_tree(|tree| {
            for v in tree.iter() {
                //println!("tree: {:#?}", &v);
                let out = range_map.get(v.as_ref().value);
                // if out.is_none() {
                // range_map.tree.with_tree(|range_tree| {
                //println!("range_tree: {:#?}", range_tree);
                // });
                // }

                let out = out.unwrap();
                //println!("vs \nindexMap: {:#?}", &out);
                assert_eq!(v.as_ref().value, out.start);
                let leaf = v.0.leaf.as_ptr() as usize;
                let out_ptr = out.value.as_ptr() as usize;
                assert_eq!(out_ptr, leaf);
            }
        });

        range_map.tree.with_tree(|range_tree| {
            for x in range_tree.iter() {
                unsafe {
                    let leaf = x.as_ref().value.value.as_ref();
                    let value = leaf.children.iter().find(|v| v.value == x.as_ref().index);
                    if value.is_some() {
                        assert!(!leaf.is_deleted());
                    }
                }
            }
        })

        //println!("========================================================================");
    }
}

prop_compose! {
    fn gen_interaction()(
            _type in 0..2,
            from in 0..100,
            len in 1..10,
        ) -> Interaction {
        if _type == 0 {
            Interaction::Insert {
                from: from as usize,
                len: len as usize,
            }
        } else {
            Interaction::Delete {
                from: from as usize,
                len: len as usize,
            }
        }
    }
}

use Interaction::*;

#[test]
fn issue_0() {
    test(&[
        Interaction::Insert { from: 0, len: 1 },
        Interaction::Insert { from: 0, len: 2 },
    ]);
}

#[test]
fn issue_1() {
    test(&[
        Interaction::Insert { from: 0, len: 3 },
        Interaction::Insert { from: 1, len: 1 },
    ]);
}

#[test]
fn issue_2() {
    test(&[
        Insert { from: 0, len: 5 },
        Insert { from: 0, len: 6 },
        Insert { from: 4, len: 3 },
    ])
}

#[test]
fn issue_4() {
    test(&[
        Insert { from: 0, len: 5 },
        Insert { from: 12, len: 2 },
        Insert { from: 5, len: 1 },
    ])
}

#[cfg(not(no_proptest))]
mod notify_proptest {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_notify(
            interactions in prop::collection::vec(gen_interaction(), 1..100),
        ) {
            test(&interactions);
        }
    }
}
