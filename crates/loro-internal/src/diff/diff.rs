//! This file was copied from the similar crate at
//! https://github.com/mitsuhiko/similar/blob/2b31f65445df9093ba007ca5a5ae6a71b899d491/src/algorithms/myers.rs
//! The original license is in the LICENSE file in the same directory as this file
//!
//! This file has been optimized for constant performance, removed some unnecessary cases
//! and simplified the use of DiffHandler.
//! Myers' diff algorithm.
//!
//! * time: `O((N+M)D)`
//! * space `O(N+M)`
//!  
//! See [the original article by Eugene W. Myers](http://www.xmailserver.org/diff2.pdf)
//! describing it.
//!
//! The implementation of this algorithm is based on the implementation by
//! Brandon Williams.
use std::ops::{Index, IndexMut, Range};

/// Utility function to check if a range is empty that works on older rust versions
#[inline(always)]
fn is_empty_range(range: &Range<usize>) -> bool {
    !(range.start < range.end)
}

#[inline(always)]
fn is_not_empty_range(range: &Range<usize>) -> bool {
    range.start < range.end
}

fn common_prefix_len<T: PartialEq>(
    old: &[T],
    old_range: Range<usize>,
    new: &[T],
    new_range: Range<usize>,
) -> usize {
    if is_empty_range(&old_range) || is_empty_range(&new_range) {
        return 0;
    }
    new_range
        .zip(old_range)
        .take_while(
            #[inline(always)]
            |x| new[x.0] == old[x.1],
        )
        .count()
}

fn common_suffix_len<T: PartialEq>(
    old: &[T],
    old_range: Range<usize>,
    new: &[T],
    new_range: Range<usize>,
) -> usize {
    if is_empty_range(&old_range) || is_empty_range(&new_range) {
        return 0;
    }
    new_range
        .rev()
        .zip(old_range.rev())
        .take_while(
            #[inline(always)]
            |x| new[x.0] == old[x.1],
        )
        .count()
}

pub(crate) trait DiffHandler {
    fn insert(&self, old_index: usize, new_index: usize, new_len: usize) -> ();
    fn delete(&self, old_index: usize, old_len: usize) -> ();
    fn replace(&self, old_index: usize, old_len: usize, new_index: usize, new_len: usize) -> ();
}

#[derive(Debug)]
pub(crate) struct OperateProxy<D: DiffHandler> {
    handler: D,
    offset: isize,
    del: Option<(usize, usize)>,
    ins: Option<(usize, usize, usize)>,
}

impl<D: DiffHandler> OperateProxy<D> {
    pub fn new(handler: D) -> Self {
        Self {
            handler,
            offset: 0,
            del: None,
            ins: None,
        }
    }

    pub fn flush_del_ins(&mut self) -> () {
        if let Some((del_old_index, del_old_len)) = self.del.take() {
            if let Some((_, ins_new_index, ins_new_len)) = self.ins.take() {
                self.handler.replace(
                    (del_old_index as isize + self.offset) as usize,
                    del_old_len,
                    ins_new_index,
                    ins_new_len,
                );
                self.offset = self.offset + ins_new_len as isize - del_old_len as isize;
            } else {
                self.handler
                    .delete((del_old_index as isize + self.offset) as usize, del_old_len);
                self.offset = self.offset - del_old_len as isize;
            }
        } else if let Some((ins_old_index, ins_new_index, ins_new_len)) = self.ins.take() {
            self.handler.insert(
                (ins_old_index as isize + self.offset) as usize,
                ins_new_index,
                ins_new_len,
            );
            self.offset = self.offset + ins_new_len as isize;
        }
    }

    pub fn delete(&mut self, old_index: usize, old_len: usize) -> () {
        if let Some((del_old_index, del_old_len)) = self.del.take() {
            self.del = Some((del_old_index, del_old_len + old_len));
        } else {
            self.del = Some((old_index, old_len));
        }
    }

    pub fn insert(&mut self, old_index: usize, new_index: usize, new_len: usize) -> () {
        self.ins = if let Some((ins_old_index, ins_new_index, ins_new_len)) = self.ins.take() {
            Some((ins_old_index, ins_new_index, new_len + ins_new_len))
        } else {
            Some((old_index, new_index, new_len))
        };
    }
}

pub(crate) fn diff<D: DiffHandler, T: PartialEq>(
    proxy: &mut OperateProxy<D>,
    old: &[T],
    new: &[T],
) -> () {
    let max_d = (old.len() + new.len() + 1) / 2 + 1;
    let mut vb = OffsetVec::new(max_d);
    let mut vf = OffsetVec::new(max_d);
    conquer(
        proxy,
        old,
        0..old.len(),
        new,
        0..new.len(),
        &mut vf,
        &mut vb,
    );
    proxy.flush_del_ins();
}

struct OffsetVec(isize, Vec<usize>);

impl OffsetVec {
    fn new(max_d: usize) -> Self {
        Self(max_d as isize, vec![0; max_d << 1])
    }

    fn len(&self) -> usize {
        self.1.len()
    }
}

impl Index<isize> for OffsetVec {
    type Output = usize;
    fn index(&self, index: isize) -> &Self::Output {
        &self.1[(index + self.0) as usize]
    }
}

impl IndexMut<isize> for OffsetVec {
    fn index_mut(&mut self, index: isize) -> &mut Self::Output {
        &mut self.1[(index + self.0) as usize]
    }
}

#[inline(always)]
fn split_at(range: Range<usize>, at: usize) -> (Range<usize>, Range<usize>) {
    (range.start..at, at..range.end)
}

fn find_middle_snake<T: PartialEq>(
    old: &[T],
    old_range: Range<usize>,
    new: &[T],
    new_range: Range<usize>,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) -> Option<(usize, usize)> {
    let n = old_range.len();
    let m = new_range.len();
    let delta = n as isize - m as isize;
    let odd = delta & 1 != 0;
    vf[1] = 0;
    vb[1] = 0;
    let d_max = (n + m + 1) / 2 + 1;
    assert!(vf.len() >= d_max);
    assert!(vb.len() >= d_max);

    for d in 0..d_max as isize {
        for k in (-d..=d).rev().step_by(2) {
            let mut x = if k == -d || (k != d && vf[k - 1] < vf[k + 1]) {
                vf[k + 1]
            } else {
                vf[k - 1] + 1
            };
            let y = (x as isize - k) as usize;
            let (x0, y0) = (x, y);
            if x < n && y < m {
                let advance = common_prefix_len(
                    old,
                    old_range.start + x..old_range.end,
                    new,
                    new_range.start + y..new_range.end,
                );
                x += advance;
            }
            vf[k] = x;
            if odd && (k - delta).abs() <= (d - 1) && vf[k] + vb[delta - k] >= n {
                return Some((x0 + old_range.start, y0 + new_range.start));
            }
        }
        for k in (-d..=d).rev().step_by(2) {
            let mut x = if k == -d || (k != d && vb[k - 1] < vb[k + 1]) {
                vb[k + 1]
            } else {
                vb[k - 1] + 1
            };
            let mut y = (x as isize - k) as usize;
            if x < n && y < m {
                let advance = common_suffix_len(
                    old,
                    old_range.start..old_range.start + n - x,
                    new,
                    new_range.start..new_range.start + m - y,
                );
                x += advance;
                y += advance;
            }
            vb[k] = x;
            if !odd && (k - delta).abs() <= d && vb[k] + vf[delta - k] >= n {
                return Some((n - x + old_range.start, m - y + new_range.start));
            }
        }
    }
    None
}

fn conquer<D: DiffHandler, T: PartialEq>(
    proxy: &mut OperateProxy<D>,
    old: &[T],
    mut old_range: Range<usize>,
    new: &[T],
    mut new_range: Range<usize>,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) -> () {
    let common_prefix_len = common_prefix_len(old, old_range.clone(), new, new_range.clone());
    if common_prefix_len > 0 {
        proxy.flush_del_ins();
        old_range.start += common_prefix_len;
        new_range.start += common_prefix_len;
    }

    let common_suffix_len = common_suffix_len(old, old_range.clone(), new, new_range.clone());
    old_range.end -= common_suffix_len;
    new_range.end -= common_suffix_len;

    if is_not_empty_range(&old_range) || is_not_empty_range(&new_range) {
        if is_empty_range(&new_range) {
            proxy.delete(old_range.start, old_range.len());
        } else if is_empty_range(&old_range) {
            proxy.insert(old_range.start, new_range.start, new_range.len());
        } else if let Some((x_start, y_start)) =
            find_middle_snake(old, old_range.clone(), new, new_range.clone(), vf, vb)
        {
            let (old_a, old_b) = split_at(old_range, x_start);
            let (new_a, new_b) = split_at(new_range, y_start);
            conquer(proxy, old, old_a, new, new_a, vf, vb);
            conquer(proxy, old, old_b, new, new_b, vf, vb);
        } else {
            proxy.delete(old_range.start, old_range.end - old_range.start);
            proxy.insert(
                old_range.start,
                new_range.start,
                new_range.end - new_range.start,
            );
        }
    }

    if common_suffix_len > 0 {
        proxy.flush_del_ins();
    }
}
