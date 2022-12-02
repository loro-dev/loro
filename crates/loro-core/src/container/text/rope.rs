use std::ops::{Deref, DerefMut};

use rle::{rle_tree::node::Node, HasLength, RleTree};

use super::{
    string_pool::PoolString,
    unicode::{find_pos_internal, find_pos_leaf, TextLength, UnicodeTreeTrait},
};

type A = UnicodeTreeTrait<16>;
type Inner = RleTree<PoolString, A>;

#[derive(Debug, Default)]
pub(super) struct Rope(Inner);

impl Deref for Rope {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Rope {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Rope {
    /// convert index from utf16 to utf8
    pub fn utf16_to_utf8(&self, index: usize) -> usize {
        self.process_cursor_at(
            index,
            |x| x.utf16.unwrap() as usize,
            |x| x.utf16_length.unwrap() as usize,
            |x| x.utf8 as usize,
            |x| x.atom_len(),
            |s, src_offset| s.utf16_index_to_utf8(src_offset),
        )
    }

    pub fn utf8_to_utf16(&self, index: usize) -> usize {
        self.process_cursor_at(
            index,
            |x| x.utf8 as usize,
            |x| x.atom_len(),
            |x| x.utf16.unwrap() as usize,
            |x| x.utf16_length.unwrap() as usize,
            |s, src_offset| s.utf8_index_to_utf16(src_offset),
        )
    }

    #[inline(always)]
    fn process_cursor_at<F, G, H, I, J>(
        &self,
        mut index: usize,
        src_cache: F,
        src_str: G,
        dst_cache: H,
        dst_str: I,
        calc_src_offset: J,
    ) -> usize
    where
        F: Fn(TextLength) -> usize,
        G: Fn(&PoolString) -> usize,
        H: Fn(TextLength) -> usize,
        I: Fn(&PoolString) -> usize,
        J: Fn(&PoolString, usize) -> usize,
    {
        self.0.with_node(|root| {
            let mut node = &**root;
            let mut ans = 0;
            loop {
                match node {
                    Node::Internal(internal_node) => {
                        let result = find_pos_internal(internal_node, index, &src_cache);
                        if !result.found {
                            unreachable!();
                        }

                        node = &internal_node.children()[result.child_index];
                        index = result.offset;
                        ans += internal_node.children()[0..result.child_index]
                            .iter()
                            .map(|x| {
                                dst_cache(match &**x {
                                    Node::Internal(x) => x.cache.text_len,
                                    Node::Leaf(x) => x.cache.text_len,
                                })
                            })
                            .sum::<usize>();
                    }
                    Node::Leaf(leaf) => {
                        let result = find_pos_leaf(leaf, index, &src_str);
                        if !result.found {
                            unreachable!();
                        }

                        ans += leaf.children()[..result.child_index]
                            .iter()
                            .map(dst_str)
                            .sum::<usize>();
                        if result.offset != 0 {
                            ans += calc_src_offset(
                                &leaf.children()[result.child_index],
                                result.offset,
                            );
                        }

                        return ans;
                    }
                }
            }
        })
    }
}
