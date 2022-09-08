use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
};

use crate::{rle_tree::tree_trait::CumulateTreeTrait, HasLength, Mergable, RleTree, Sliceable};

#[derive(Debug)]
struct CustomString(String);
impl Deref for CustomString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CustomString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl HasLength for CustomString {
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl Mergable for CustomString {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        self.len() + other.len() < 64
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        self.push_str(other.as_str())
    }
}

type StringTreeTrait = CumulateTreeTrait<CustomString, 4>;

impl Sliceable for CustomString {
    fn slice(&self, from: usize, to: usize) -> Self {
        CustomString(self.0.slice(from, to))
    }
}

impl Display for RleTree<CustomString, StringTreeTrait> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.with_tree(|tree| {
            for s in tree.iter() {
                f.write_str(s.as_ref().0.as_str())?;
            }

            Ok(())
        })
    }
}

impl From<String> for CustomString {
    fn from(origin: String) -> Self {
        CustomString(origin)
    }
}

impl From<&str> for CustomString {
    fn from(origin: &str) -> Self {
        CustomString(origin.to_owned())
    }
}

#[test]
fn basic_string_op() {
    let mut tree: RleTree<CustomString, StringTreeTrait> = RleTree::default();
    tree.with_tree_mut(|tree| {
        tree.insert(0, "test".into());
        tree.insert(0, "hello ".into());
    });
    let m = format!("{}", tree);
    assert_eq!(m, "hello test");
}

#[test]
fn issue_0() {
    let mut tree: RleTree<CustomString, StringTreeTrait> = RleTree::default();
    let insert_keys = "0123456789abcdefghijklmnopq";
    tree.with_tree_mut(|tree| {
        for i in 0..(1e6 as usize) {
            let start = i % insert_keys.len();
            if i % 3 == 0 && tree.len() > 0 {
                let start = i % tree.len();
                let len = (i * i) % std::cmp::min(tree.len(), 10);
                let end = std::cmp::min(start + len, tree.len());
                if start == end {
                    continue;
                }
                tree.delete_range(Some(start), Some(end));
            } else if tree.len() == 0 {
                tree.insert(0, insert_keys[start..start + 1].to_string().into());
            } else {
                tree.insert(
                    i % tree.len(),
                    insert_keys[start..start + 1].to_string().into(),
                );
            }

            tree.debug_check();
        }
    });
}

#[derive(enum_as_inner::EnumAsInner, Debug)]
enum Interaction {
    Insert { insert_at: usize, content: String },
    Delete { from: usize, len: usize },
}

impl Interaction {
    pub fn test_assert(&self, s: &mut String, tree: &mut RleTree<CustomString, StringTreeTrait>) {
        self.apply_to_str(s);
        self.apply_to_tree(tree);
        assert_eq!(&tree.to_string(), s);
    }

    fn apply_to_str(&self, s: &mut String) {
        match self {
            Interaction::Insert { insert_at, content } => {
                let insert_at = *insert_at % (s.len() + 1);
                s.insert_str(insert_at, content.as_str())
            }
            Interaction::Delete { from, len } => {
                let from = *from % s.len();
                let mut to = from + (*len % s.len());
                if to > s.len() {
                    to = s.len();
                }

                s.drain(from..to);
            }
        }
    }

    fn apply_to_tree(&self, tree: &mut RleTree<CustomString, StringTreeTrait>) {
        match self {
            Interaction::Insert { insert_at, content } => {
                tree.with_tree_mut(|tree| {
                    let insert_at = *insert_at % (tree.len() + 1);
                    tree.insert(insert_at, content.clone().into());
                });
            }
            Interaction::Delete { from, len } => {
                tree.with_tree_mut(|tree| {
                    let from = *from % tree.len();
                    let mut to = from + (*len % tree.len());
                    if to > tree.len() {
                        to = tree.len();
                    }

                    tree.delete_range(Some(from), Some(to));
                });
            }
        }
    }
}

#[cfg(not(no_proptest))]
mod string_prop_test {
    use super::*;
    use proptest::prelude::*;

    prop_compose! {
        fn gen_interaction()(
                _type in 0..1,
                from in 0..10000000,
                len in 0..10,
                content in "[a-z]*"
            ) -> Interaction {
            if _type == 0 {
                Interaction::Insert {
                    insert_at: from as usize,
                    content
                }
            } else {
                Interaction::Delete {
                    from: from as usize,
                    len: len as usize,
                }
            }
        }
    }

    proptest! {
        #[test]
        fn test_tree_string_op_the_same(
            interactions in prop::collection::vec(gen_interaction(), 1..100),
        ) {
            let mut s = String::new();
            let mut tree = RleTree::default();
            for interaction in interactions {
                interaction.test_assert(&mut s, &mut tree);
                tree.with_tree_mut(|tree|tree.debug_check());
            }
        }
    }

    #[cfg(slow_proptest)]
    proptest! {
        #[test]
        fn test_tree_string_op_the_same_slow(
            interactions in prop::collection::vec(gen_interaction(), 1..2000),
        ) {
            let mut s = String::new();
            let mut tree = RleTree::default();
            for interaction in interactions {
                interaction.test_assert(&mut s, &mut tree);
            }
        }
    }
}
