use std::collections::VecDeque;

use rle::{
    rle_tree::{iter, node::Node},
    HasLength, RleTree,
};

use crate::LoroError;

use super::{
    string_pool::PoolString,
    unicode::{find_pos_internal, find_pos_leaf, TextLength, UnicodeTreeTrait},
};

type A = UnicodeTreeTrait<16>;
type Inner = RleTree<PoolString, A>;

#[derive(Debug, Default)]
pub(super) struct Rope {
    inner: Inner,
    cache: Utf16Cache,
}

impl Rope {
    /// convert index from utf16 to utf8
    pub fn utf16_to_utf8(&self, index: usize) -> usize {
        self.process_cursor_at(
            index,
            |x| x.utf16 as usize,
            |x| x.utf16_length as usize,
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
            |x| x.utf16 as usize,
            |x| x.utf16_length as usize,
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
        self.inner.with_node(|root| {
            let mut node = &**root;
            let mut ans = 0;
            loop {
                match node {
                    Node::Internal(internal_node) => {
                        if index == 0 {
                            assert_eq!(ans, 0);
                            return 0;
                        }
                        let result = find_pos_internal(internal_node, index, &src_cache);
                        if !result.found {
                            unreachable!();
                        }

                        node = &internal_node.children()[result.child_index].node;
                        index = result.offset;
                        ans += internal_node.children()[0..result.child_index]
                            .iter()
                            .map(|x| dst_cache(x.parent_cache))
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

    pub fn insert(&mut self, pos: usize, value: PoolString) {
        self.cache.clear();
        self.inner.insert(pos, value);
    }

    pub fn insert_utf16(
        &mut self,
        utf16_pos: usize,
        value: PoolString,
    ) -> Result<usize, LoroError> {
        if utf16_pos > self.utf16_len() {
            return Err(LoroError::OutOfBound {
                pos: utf16_pos,
                len: self.utf16_len(),
            });
        }

        if let Some(utf8_pos) = self.cache.get_utf8_from_utf16(utf16_pos) {
            self.cache.update_on_insert_op(
                utf8_pos,
                utf16_pos,
                value.atom_len(),
                value.utf16_length as usize,
            );
            self.inner.insert(utf8_pos, value);
            Ok(utf8_pos)
        } else {
            let utf8_pos = self.utf16_to_utf8(utf16_pos);
            self.inner.insert(utf8_pos, value);
            Ok(utf8_pos)
        }
    }

    pub fn delete_range(&mut self, pos: Option<usize>, end: Option<usize>) {
        self.cache.clear();
        self.inner.delete_range(pos, end);
    }

    pub fn delete_utf16(
        &mut self,
        utf16_pos: usize,
        utf16_len: usize,
    ) -> Result<(usize, usize), LoroError> {
        if utf16_pos + utf16_len > self.utf16_len() {
            dbg!(self.len(), self.utf16_len());
            return Err(LoroError::OutOfBound {
                pos: utf16_len + utf16_pos,
                len: self.utf16_len(),
            });
        }

        let utf8_pos = self
            .cache
            .get_utf8_from_utf16(utf16_pos)
            .unwrap_or_else(|| self.utf16_to_utf8(utf16_pos));
        let utf8_end = self
            .cache
            .get_utf8_from_utf16(utf16_pos + utf16_len)
            .unwrap_or_else(|| self.utf16_to_utf8(utf16_pos + utf16_len));
        self.inner.delete_range(Some(utf8_pos), Some(utf8_end));
        let utf8_len = utf8_end - utf8_pos;
        self.cache
            .update_on_delete_op(utf8_pos, utf16_pos, utf8_len, utf16_len);
        Ok((utf8_pos, utf8_len))
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline(always)]
    pub fn utf16_len(&self) -> usize {
        self.inner.root_cache().utf16 as usize
    }

    #[inline(always)]
    pub fn debug_inspect(&mut self) {
        self.inner.debug_inspect()
    }

    #[inline(always)]
    pub fn iter(&self) -> iter::Iter<'_, PoolString, A> {
        self.inner.iter()
    }
}

const MAX_CACHE_NUM: usize = 4;

#[derive(Debug, Default)]
struct CacheItem {
    utf8_pos: usize,
    utf16_pos: usize,
}

#[derive(Debug, Default)]
struct Utf16Cache {
    caches: VecDeque<CacheItem>,
}

impl Utf16Cache {
    pub fn update_on_insert_op(
        &mut self,
        utf8_pos: usize,
        utf16_pos: usize,
        utf8_len: usize,
        utf16_len: usize,
    ) {
        for item in self.caches.iter_mut() {
            if item.utf8_pos < utf8_pos {
                debug_assert!(item.utf16_pos < utf16_pos);
                continue;
            }

            item.utf8_pos += utf8_len;
            item.utf16_pos += utf16_len;
        }

        self.push(CacheItem {
            utf8_pos,
            utf16_pos,
        });
        self.push(CacheItem {
            utf8_pos: utf8_pos + utf8_len,
            utf16_pos: utf16_pos + utf16_len,
        });
    }

    pub fn update_on_delete_op(
        &mut self,
        utf8_pos: usize,
        utf16_pos: usize,
        utf8_len: usize,
        utf16_len: usize,
    ) {
        for item in self.caches.iter_mut() {
            if item.utf8_pos < utf8_pos {
                debug_assert!(item.utf16_pos < utf16_pos);
                continue;
            }

            if item.utf8_pos < utf8_pos + utf8_len {
                item.utf8_pos = utf8_pos;
                item.utf16_pos = utf16_pos;
            } else {
                item.utf8_pos -= utf8_len;
                item.utf16_pos -= utf16_len;
            }
        }

        self.push(CacheItem {
            utf8_pos,
            utf16_pos,
        });
    }

    pub fn get_utf8_from_utf16(&self, utf16: usize) -> Option<usize> {
        for item in self.caches.iter() {
            if item.utf16_pos == utf16 {
                return Some(item.utf8_pos);
            }
        }

        None
    }

    pub fn get_utf16_from_utf8(&self, utf8: usize) -> Option<usize> {
        for item in self.caches.iter() {
            if item.utf8_pos == utf8 {
                return Some(item.utf16_pos);
            }
        }

        None
    }

    fn push(&mut self, cache: CacheItem) {
        self.caches.push_back(cache);
        if self.caches.len() > MAX_CACHE_NUM {
            self.caches.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.caches.clear();
    }
}
