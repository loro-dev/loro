use std::{
    iter::Sum,
    ops::{Add, AddAssign, Deref, Neg, Sub},
};

use rle::{
    rle_tree::{node::Node, tree_trait::FindPosResult, HeapMode, Position},
    HasLength, RleTreeTrait,
};

use super::string_pool::PoolString;

#[derive(Debug, Clone, Copy)]
pub(super) struct UnicodeTreeTrait<const SIZE: usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLength {
    pub utf8: i32,
    pub utf16: Option<i32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Cache {
    pub text_len: TextLength,
    pub unknown_elem_len: isize,
}

impl Default for TextLength {
    fn default() -> Self {
        Self {
            utf8: 0,
            utf16: Some(0),
        }
    }
}

impl Sub for TextLength {
    type Output = TextLength;

    fn sub(self, rhs: Self) -> Self::Output {
        TextLength {
            utf8: self.utf8 - rhs.utf8,
            utf16: match (self.utf16, rhs.utf16) {
                (Some(u), Some(o)) => Some(u - o),
                _ => None,
            },
        }
    }
}

impl Neg for TextLength {
    type Output = TextLength;

    fn neg(self) -> Self::Output {
        TextLength {
            utf8: -self.utf8,
            utf16: self.utf16.map(|x| -x),
        }
    }
}

impl Neg for Cache {
    type Output = Cache;

    fn neg(self) -> Self::Output {
        Self {
            text_len: -self.text_len,
            unknown_elem_len: -self.unknown_elem_len,
        }
    }
}

impl Sub for Cache {
    type Output = Cache;

    fn sub(self, rhs: Self) -> Self::Output {
        Cache {
            text_len: self.text_len - rhs.text_len,
            unknown_elem_len: self.unknown_elem_len - rhs.unknown_elem_len,
        }
    }
}

impl Add for TextLength {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        TextLength {
            utf8: self.utf8 + rhs.utf8,
            utf16: if let (Some(a), Some(b)) = (self.utf16, rhs.utf16) {
                Some(a + b)
            } else {
                None
            },
        }
    }
}

impl AddAssign for TextLength {
    fn add_assign(&mut self, rhs: Self) {
        self.utf8 += rhs.utf8;
        match &mut self.utf16 {
            a @ Some(_) => match rhs.utf16 {
                Some(y) => {
                    *a = Some(a.unwrap() + y);
                }
                None => {
                    *a = None;
                }
            },
            None => {}
        }
    }
}

impl AddAssign for Cache {
    fn add_assign(&mut self, rhs: Self) {
        self.text_len += rhs.text_len;
        self.unknown_elem_len += rhs.unknown_elem_len;
    }
}

impl Add for Cache {
    type Output = Cache;

    fn add(self, rhs: Self) -> Self::Output {
        Cache {
            text_len: self.text_len + rhs.text_len,
            unknown_elem_len: self.unknown_elem_len + rhs.unknown_elem_len,
        }
    }
}

impl Sum for TextLength {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut u8 = 0;
        let mut u16 = Some(0);
        for item in iter {
            u8 += item.utf8;
            if let (Some(a), Some(b)) = (u16, item.utf16) {
                u16 = Some(a + b);
            } else {
                u16 = None;
            }
        }

        Self {
            utf8: u8,
            utf16: u16,
        }
    }
}

impl Sum for Cache {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut utf8 = 0;
        let mut utf16 = Some(0);
        let mut unknown_elem_len = 0;
        for item in iter {
            utf8 += item.text_len.utf8;
            unknown_elem_len += item.unknown_elem_len;
            if let (Some(a), Some(b)) = (utf16, item.text_len.utf16) {
                utf16 = Some(a + b);
            } else {
                utf16 = None;
            }
        }

        Self {
            text_len: TextLength { utf8, utf16 },
            unknown_elem_len,
        }
    }
}

impl<const SIZE: usize> RleTreeTrait<PoolString> for UnicodeTreeTrait<SIZE> {
    const MAX_CHILDREN_NUM: usize = SIZE;

    type Int = usize;

    type CacheUpdate = Cache;
    type Cache = Cache;

    type Arena = HeapMode;

    fn update_cache_leaf(
        node: &mut rle::rle_tree::node::LeafNode<'_, PoolString, Self>,
    ) -> Self::CacheUpdate {
        let old = node.cache;
        node.cache = node
            .children()
            .iter()
            .map(|x| Cache {
                text_len: x.text_len(),
                unknown_elem_len: x.range.is_unknown() as isize,
            })
            .sum();

        node.cache - old
    }

    fn update_cache_internal(
        node: &mut rle::rle_tree::node::InternalNode<'_, PoolString, Self>,
        hint: Option<Self::CacheUpdate>,
    ) -> Self::CacheUpdate {
        match hint {
            Some(diff) => {
                node.cache += diff;
                if node.cache.unknown_elem_len == 0 && node.cache.text_len.utf16.is_none() {
                    node.cache.text_len.utf16 = Some(
                        node.children()
                            .iter()
                            .map(|x| x.cache.text_len.utf16.unwrap())
                            .sum::<i32>(),
                    );
                }

                debug_assert_eq!(
                    node.children().iter().map(|x| x.cache).sum::<Cache>(),
                    node.cache,
                );

                if cfg!(test) || cfg!(debug_assert) {
                    for child in node.children() {
                        debug_assert_eq!(child.cache, child.node.cache());
                    }
                }
                diff
            }
            None => {
                let old = node.cache;
                node.cache = node.children().iter().map(|x| x.cache).sum();
                node.cache - old
            }
        }
    }

    fn find_pos_internal(
        node: &rle::rle_tree::node::InternalNode<'_, PoolString, Self>,
        index: Self::Int,
    ) -> FindPosResult<Self::Int> {
        find_pos_internal(node, index, &|x| x.utf8 as usize)
    }

    fn find_pos_leaf(
        node: &rle::rle_tree::node::LeafNode<'_, PoolString, Self>,
        index: Self::Int,
    ) -> rle::rle_tree::tree_trait::FindPosResult<usize> {
        find_pos_leaf(node, index, &|x| x.atom_len())
    }

    fn get_index(
        node: &rle::rle_tree::node::LeafNode<'_, PoolString, Self>,
        mut child_index: usize,
    ) -> Self::Int {
        debug_assert!(!node.is_deleted());
        let mut index = 0;
        for i in 0..child_index {
            index += node.children()[i].content_len();
        }

        child_index = node.get_index_in_parent().unwrap();
        // SAFETY: parent is valid if node is valid
        let mut node = unsafe { node.parent().as_ref() };
        loop {
            for i in 0..child_index {
                index += node.children()[i].cache.text_len.utf8 as usize;
            }

            if let Some(parent) = node.parent() {
                child_index = node.get_index_in_parent().unwrap();
                // SAFETY: parent is valid if node is valid
                node = unsafe { parent.as_ref() };
            } else {
                break;
            }
        }

        index
    }

    fn len_leaf(node: &rle::rle_tree::node::LeafNode<'_, PoolString, Self>) -> Self::Int {
        node.cache.text_len.utf8 as usize
    }

    fn len_internal(node: &rle::rle_tree::node::InternalNode<'_, PoolString, Self>) -> Self::Int {
        node.cache.text_len.utf8 as usize
    }

    fn cache_to_update(x: Self::Cache) -> Self::CacheUpdate {
        x
    }

    fn value_to_update(x: &PoolString) -> Self::CacheUpdate {
        Cache {
            text_len: x.text_len(),
            unknown_elem_len: x.range.is_unknown() as isize,
        }
    }
}

#[inline(always)]
pub(super) fn find_pos_internal<F, const S: usize>(
    node: &rle::rle_tree::node::InternalNode<'_, PoolString, UnicodeTreeTrait<S>>,
    mut index: usize,
    f: &F,
) -> FindPosResult<usize>
where
    F: Fn(TextLength) -> usize,
{
    if node.children().is_empty() {
        return FindPosResult::new_not_found(0, 0, Position::Before);
    }

    let mut last_len = 0;
    for (i, child) in node.children().iter().enumerate() {
        last_len = f(child.cache.text_len);
        if index <= last_len {
            return FindPosResult::new(i, index, Position::get_pos(index, last_len));
        }

        index -= last_len;
    }

    assert_eq!(index, 0);
    FindPosResult::new(node.children().len() - 1, last_len, Position::End)
}

#[inline(always)]
pub(super) fn find_pos_leaf<F, const S: usize>(
    node: &rle::rle_tree::node::LeafNode<'_, PoolString, UnicodeTreeTrait<S>>,
    mut index: usize,
    f: &F,
) -> FindPosResult<usize>
where
    F: Fn(&PoolString) -> usize,
{
    if node.children().is_empty() {
        return FindPosResult::new_not_found(0, 0, Position::Before);
    }

    for (i, child) in node.children().iter().enumerate() {
        if index < f(child) {
            return FindPosResult::new(i, index, Position::get_pos(index, f(child)));
        }

        index -= f(child);
    }

    assert_eq!(index, 0);
    FindPosResult::new(
        node.children().len() - 1,
        f(node.children().last().unwrap()),
        Position::End,
    )
}

#[cfg(any(test, feature = "test_utils"))]
pub mod test {
    use std::sync::{Arc, Mutex};

    use arbitrary::{Arbitrary, Unstructured};
    use enum_as_inner::EnumAsInner;
    use rle::RleTree;

    use crate::container::text::{
        string_pool::{PoolString, StringPool},
        text_content::SliceRange,
    };

    use super::UnicodeTreeTrait;

    #[derive(Default)]
    pub struct TestRope {
        pool: Arc<Mutex<StringPool>>,
        rope: RleTree<PoolString, UnicodeTreeTrait<4>>,
    }

    impl TestRope {
        fn insert(&mut self, pos: usize, s: &str) {
            let s = StringPool::alloc_pool_string(&self.pool, s);
            self.rope.insert(pos, s);
        }

        fn delete(&mut self, pos: usize, len: usize) {
            self.rope.delete_range(Some(pos), Some(pos + len));
        }

        fn insert_unknown(&mut self, pos: usize, len: usize) {
            self.rope.insert(
                pos,
                PoolString::from_slice(&self.pool, SliceRange::new_unknown(len as u32)),
            );
        }
    }

    #[derive(Debug, Clone, Arbitrary, EnumAsInner)]
    pub enum Action {
        Insert { pos: u16, value: u16 },
        InsertUnknown { pos: u16, len: u8 },
        Delete { pos: u16, len: u8 },
    }

    use Action::*;

    pub fn apply(actions: &mut [Action]) {
        let mut test: TestRope = Default::default();
        for action in actions.iter_mut() {
            match action {
                Action::Insert { pos, value } => {
                    *pos = (*pos as usize % (test.rope.len() + 1)) as u16;
                    debug_log::debug_log!("insert {} {}", *pos, *value);
                    test.insert(*pos as usize, &format!("{} ", value));
                }
                Action::InsertUnknown { pos, len } => {
                    *pos = (*pos as usize % (test.rope.len() + 1)) as u16;
                    debug_log::debug_log!("unknown {} {}", *pos, *len);
                    test.insert_unknown(*pos as usize, *len as usize);
                }
                Action::Delete { pos, len } => {
                    if test.rope.len() == 0 {
                        continue;
                    }

                    *pos = (*pos as usize % test.rope.len()) as u16;
                    let end = (*pos as usize + *len as usize).min(test.rope.len());
                    *len = (end - *pos as usize) as u8;
                    debug_log::debug_log!("del {} {}", *pos, *len);
                    test.delete(*pos as usize, *len as usize)
                }
            }
        }
    }

    fn normalize(actions: &mut [Action]) {
        let mut text_len = 0;
        for action in actions.iter_mut() {
            match action {
                Action::Insert { pos, value } => {
                    *pos = (*pos as usize % (text_len + 1)) as u16;
                    text_len += format!("{}", value).len();
                }
                Action::InsertUnknown { pos, len } => {
                    *pos = (*pos as usize % (text_len + 1)) as u16;
                    text_len += *len as usize;
                }
                Action::Delete { pos, len } => {
                    if text_len == 0 {
                        continue;
                    }

                    *pos = (*pos as usize % text_len) as u16;
                    let end = (*pos as usize + *len as usize).min(text_len);
                    *len = (end as u16 - *pos) as u8;
                    text_len -= *len as usize;
                }
            }
        }
    }

    fn prop(u: &mut Unstructured<'_>) -> arbitrary::Result<()> {
        let mut xs = u.arbitrary::<Vec<Action>>()?;
        normalize(&mut xs);
        if let Err(e) = std::panic::catch_unwind(|| {
            apply(&mut xs.clone());
        }) {
            dbg!(xs);
            println!("{:?}", e);
            panic!()
        } else {
            Ok(())
        }
    }

    #[test]
    fn failed_2() {
        apply(&mut [
            Insert {
                pos: 19789,
                value: 19789,
            },
            Insert {
                pos: 333,
                value: 65520,
            },
            Insert {
                pos: 19789,
                value: 41805,
            },
            Insert {
                pos: 19967,
                value: 19789,
            },
            Insert {
                pos: 33792,
                value: 2560,
            },
            InsertUnknown {
                pos: 41891,
                len: 163,
            },
            Insert {
                pos: 41805,
                value: 41891,
            },
            InsertUnknown {
                pos: 41891,
                len: 163,
            },
        ])
    }

    #[test]
    fn failed_1() {
        apply(&mut [
            InsertUnknown {
                pos: 56355,
                len: 126,
            },
            Insert { pos: 256, value: 0 },
            Insert {
                pos: 32256,
                value: 9180,
            },
            Insert {
                pos: 62475,
                value: 32500,
            },
            Delete {
                pos: 9089,
                len: 220,
            },
        ])
    }

    #[test]
    fn failed_0() {
        apply(&mut [
            Insert {
                pos: 0,
                value: 12451,
            },
            Insert {
                pos: 3,
                value: 46337,
            },
            Delete { pos: 6, len: 4 },
            InsertUnknown { pos: 0, len: 59 },
            InsertUnknown { pos: 4, len: 225 },
            InsertUnknown { pos: 73, len: 193 },
        ])
    }

    #[test]
    fn arb_apply() {
        arbtest::builder().budget_ms(200).run(prop)
    }

    #[test]
    fn random_op() {
        let mut test: TestRope = Default::default();
        test.insert(0, "123456789");
        test.delete(7, 1);
        test.delete(5, 1);
        test.delete(3, 1);
        test.delete(1, 1);
        for _ in 0..100 {
            test.insert(0, "1");
        }
        while test.rope.len() > 0 {
            test.delete(0, 1);
        }
    }

    #[test]
    fn random_op_2() {
        let mut test: TestRope = Default::default();
        test.insert(0, "123456789");
        for i in 0..100 {
            test.insert(i, "1234");
        }
        while test.rope.len() > 100 {
            test.delete(5, 40);
        }
    }

    #[test]
    fn case_0() {
        let mut test: TestRope = Default::default();
        test.insert(0, "35624");
        test.delete(0, 5);
        test.insert(0, "35108");
    }
}
