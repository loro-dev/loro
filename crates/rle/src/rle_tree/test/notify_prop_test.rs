use num::Zero;
use proptest::prop_compose;
use rand::{rngs::StdRng, SeedableRng};

use crate::{
    range_map::{RangeMap, WithStartEnd},
    rle_trait::ZeroElement,
    rle_tree::tree_trait::CumulateTreeTrait,
    HasLength, Mergable, Sliceable,
};

use super::super::*;
use std::{ops::Deref, ptr::NonNull};

type Value = WithStartEnd<usize, u64>;
type ValueTreeTrait = CumulateTreeTrait<Value, 4>;

#[derive(enum_as_inner::EnumAsInner, Debug)]
enum Interaction {
    Insert {
        from: usize,
        len: usize,
        use_cursor: bool,
    },
    Delete {
        from: usize,
        len: usize,
    },
}

macro_rules! _println {
    ($($arg:tt)*) => {{
        // println!($($arg)*);
    }};
}

macro_rules! _dbg {
    ($($arg:tt)*) => {{
        // dbg!($($arg)*);
    }};
}

impl Interaction {
    fn apply<F, R>(&self, tree: &mut RleTree<Value, ValueTreeTrait>, rng: &mut R, notify: &mut F)
    where
        F: FnMut(&Value, *mut LeafNode<'_, Value, ValueTreeTrait>),
        R: rand::Rng,
    {
        match self {
            Interaction::Insert {
                from,
                len,
                use_cursor,
            } => {
                let mut from = *from;
                let len = *len;
                if tree.len() == 0 {
                    from = 0;
                } else {
                    from %= tree.len();
                }
                let value = WithStartEnd {
                    start: 0,
                    end: len,
                    value: rng.next_u64(),
                };
                _println!("Insert {{from: {}, len: {}}},", from, len);
                if *use_cursor {
                    if let Some(cursor) = tree.get_mut(from) {
                        cursor.insert_before_notify(value, notify)
                    } else {
                        tree.insert_notify(from, value, notify);
                    }
                } else {
                    tree.insert_notify(from, value, notify);
                }
            }
            Interaction::Delete { from, len } => {
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
                _println!("Delete {{from: {}, len: {} }},", from, len);
                tree.delete_range_notify(Some(from), Some(from + len), notify)
            }
        }
    }
}

impl Sliceable for u64 {
    fn slice(&self, from: usize, _to: usize) -> Self {
        self + from as u64
    }
}

#[derive(Debug)]
struct MyNonNull<T>(NonNull<T>, usize);

impl<T> PartialEq for MyNonNull<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}

impl<T> Eq for MyNonNull<T> {}
impl<T> Default for MyNonNull<T> {
    fn default() -> Self {
        Self(NonNull::dangling(), 0)
    }
}

impl<T> Clone for MyNonNull<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl<T> Copy for MyNonNull<T> {}

impl<T> Deref for MyNonNull<T> {
    type Target = NonNull<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Sliceable for MyNonNull<T> {
    fn slice(&self, from: usize, to: usize) -> Self {
        Self(self.0, to - from)
    }
}

impl<T> From<NonNull<T>> for MyNonNull<T> {
    fn from(value: NonNull<T>) -> Self {
        Self(value, 1)
    }
}

impl<T> Mergable for MyNonNull<T> {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}

impl<T> HasLength for MyNonNull<T> {
    fn len(&self) -> usize {
        self.1
    }
}

fn test(interactions: &[Interaction]) {
    let mut tree: RleTree<Value, ValueTreeTrait> = Default::default();
    let mut rng = StdRng::seed_from_u64(123);
    type ValueIndex<'a> = WithStartEnd<u64, MyNonNull<LeafNode<'a, Value, ValueTreeTrait>>>;
    let mut range_map: RangeMap<u64, ValueIndex<'_>> = Default::default();
    for interaction in interactions.iter() {
        let mut func = |value: &Value, node: *mut LeafNode<'_, Value, ValueTreeTrait>| {
            // SAFETY: this is safe because node must be valid
            let ptr = unsafe { NonNull::new_unchecked(node as usize as *mut _) };
            range_map.set(
                value.value,
                WithStartEnd::new(value.value, value.value + value.len() as u64, ptr.into()),
            );
            _println!("notify Value: {:?}, Ptr: {:#016x}", value, node as usize);
        };
        interaction.apply(&mut tree, &mut rng, &mut func);
        _dbg!(&tree);
        _println!("range_tree: {:#?}", range_tree);

        for origin_cursor in tree.iter() {
            // println!("tree: {:#?}", &v);
            let origin_value = origin_cursor.as_ref();
            let id = origin_value.value;
            let range_map_output = range_map.get(id);

            if range_map_output.is_none() {
                dbg!(origin_value);
            }

            let range_map_out = range_map_output.unwrap();
            let range = range_map_out.start..range_map_out.end;
            assert!(
                (origin_value.len() == 0 && origin_value.value == range.start)
                    || (range.contains(&id)
                        && range.contains(&(origin_value.value + origin_value.len() as u64 - 1))),
                "origin={:#?}, range={:#?}",
                origin_value,
                range
            );
            // SAFETY: this is a test
            assert!(!unsafe { origin_cursor.0.leaf.as_ref().is_deleted() });
            let origin_leaf_ptr = origin_cursor.0.leaf.as_ptr() as usize;
            let range_map_ptr = range_map_out.value.as_ptr() as usize;
            assert_eq!(
                range_map_ptr,
                origin_leaf_ptr,
                "id: {}; [PTR] actual: {:#016x} vs expected: {:#016x}",
                origin_cursor.as_ref().value,
                range_map_ptr,
                origin_leaf_ptr
            );
        }

        _println!("========================================================================");
    }
}

prop_compose! {
    fn gen_interaction()(
            _type in 0..2,
            from in 0..1000,
            len in 0..10,
            use_cursor: bool,
        ) -> Interaction {
        if _type == 0 {
            Interaction::Insert {
                from: from as usize,
                len: len as usize,
                use_cursor
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
        Interaction::Insert {
            from: 0,
            len: 1,
            use_cursor: false,
        },
        Interaction::Insert {
            from: 0,
            len: 2,
            use_cursor: false,
        },
    ]);
}

#[test]
fn issue_1() {
    test(&[
        Interaction::Insert {
            from: 0,
            len: 3,
            use_cursor: false,
        },
        Interaction::Insert {
            from: 1,
            len: 1,
            use_cursor: false,
        },
    ]);
}

#[test]
fn issue_2() {
    test(&[
        Insert {
            from: 0,
            len: 5,
            use_cursor: false,
        },
        Insert {
            from: 0,
            len: 6,
            use_cursor: false,
        },
        Insert {
            from: 4,
            len: 3,
            use_cursor: false,
        },
    ])
}

#[test]
fn issue_4() {
    test(&[
        // 0-5
        Insert {
            from: 0,
            len: 5,
            use_cursor: false,
        },
        // 0-2, 2-4, 2-5
        Insert {
            from: 2,
            len: 2,
            use_cursor: true,
        },
        // 0-2, 2-4, 2-3, 5-6, 3-5
        Insert {
            from: 5,
            len: 1,
            use_cursor: true,
        },
    ])
}

#[test]
fn issue_5() {
    test(&[
        Insert {
            from: 0,
            len: 0,
            use_cursor: false,
        },
        Delete { from: 0, len: 0 },
        Delete { from: 0, len: 0 },
    ])
}

#[cfg(not(no_proptest))]
mod notify_proptest {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_notify(
            interactions in prop::collection::vec(gen_interaction(), 1..1000),
        ) {
            test(&interactions);
        }
    }
}
