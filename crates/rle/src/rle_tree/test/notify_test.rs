use proptest::prop_compose;

use crate::{range_map::RangeMap, rle_tree::tree_trait::CumulateTreeTrait};

use super::super::*;
use std::{ops::Range, ptr::NonNull};

type RangeTreeTrait = CumulateTreeTrait<Range<usize>, 4>;

#[derive(enum_as_inner::EnumAsInner, Debug)]
enum Interaction {
    Insert { from: usize, len: usize },
    Delete { from: usize, len: usize },
}

impl Interaction {
    fn apply<F>(&self, tree: &mut RleTree<Range<usize>, RangeTreeTrait>, notify: &mut F)
    where
        F: FnMut(&Range<usize>, *mut LeafNode<'_, Range<usize>, RangeTreeTrait>),
    {
        match self {
            Interaction::Insert { from, len } => {
                tree.with_tree_mut(|tree| tree.insert_notify(*from, *from..*from + *len, notify))
            }
            Interaction::Delete { from, len } => tree.with_tree_mut(|tree| {
                tree.delete_range_notify(Some(*from), Some(*from + *len), notify)
            }),
        }
    }
}

fn test(tree: &mut RleTree<Range<usize>, RangeTreeTrait>, interactions: &[Interaction]) {
    // let mut range_map: RangeMap<usize, NonNull<LeafNode<Range<usize>, RangeTreeTrait>>> =
    //     Default::default();
    // let mut func = |range: &Range<usize>, node: *mut LeafNode<'_, Range<usize>, RangeTreeTrait>| {
    //     let ptr = unsafe { NonNull::new_unchecked(node as usize as *mut _) };
    //     range_map.set(range.start, ptr);
    // };
    // for interaction in interactions.iter() {
    //     interaction.apply(tree, &mut func);
    // }
}

prop_compose! {
    fn gen_interaction()(
            _type in 0..1,
            from in 0..10000,
            len in 0..10,
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
