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
use std::iter::zip;
use std::ops::{Index, IndexMut};

/// Utility function to check if a range is empty that works on older rust versions
#[inline(always)]
fn is_empty_range(start: usize, end: usize) -> bool {
    !(start < end)
}

#[inline(always)]
fn is_not_empty_range(start: usize, end: usize) -> bool {
    start < end
}

fn common_prefix(xs: &[char], ys: &[char]) -> usize {
    let chunk_size = 4;
    let off = zip(xs.chunks_exact(chunk_size), ys.chunks_exact(chunk_size))
        .take_while(|(xs_chunk, ys_chunk)| xs_chunk == ys_chunk)
        .count()
        * chunk_size;
    off + zip(&xs[off..], &ys[off..])
        .take_while(|(x, y)| x == y)
        .count()
}

fn common_suffix_len(old: &[char], new: &[char]) -> usize {
    let chunk_size = 4;
    let old_len = old.len();
    let new_len = new.len();

    let off = zip(old.rchunks_exact(chunk_size), new.rchunks_exact(chunk_size))
        .take_while(|(old_chunk, new_chunk)| old_chunk == new_chunk)
        .count()
        * chunk_size;

    off + zip(
        old[..old_len - off].iter().rev(),
        new[..new_len - off].iter().rev(),
    )
    .take_while(|(o, n)| o == n)
    .count()
}

pub(crate) trait DiffHandler {
    fn insert(&mut self, old_index: usize, new_index: usize, new_len: usize);
    fn delete(&mut self, old_index: usize, old_len: usize);
    fn replace(&mut self, old_index: usize, old_len: usize, new_index: usize, new_len: usize);
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

pub(crate) fn myers_diff<D: DiffHandler>(
    proxy: &mut OperateProxy<D>,
    old: &[char],
    new: &[char],
) -> () {
    let max_d = (old.len() + new.len() + 1) / 2 + 1;
    let mut vb = OffsetVec::new(max_d);
    let mut vf = OffsetVec::new(max_d);
    conquer(
        proxy,
        old,
        0,
        old.len(),
        new,
        0,
        new.len(),
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

fn find_middle_snake(
    old: &[char],
    old_start: usize,
    old_end: usize,
    new: &[char],
    new_start: usize,
    new_end: usize,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) -> Option<(usize, usize)> {
    let n = old_end - old_start;
    let m = new_end - new_start;
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
                let advance =
                    common_prefix(&old[old_start + x..old_end], &new[new_start + y..new_end]);
                x += advance;
            }
            vf[k] = x;
            if odd && (k - delta).abs() <= (d - 1) && vf[k] + vb[delta - k] >= n {
                return Some((x0 + old_start, y0 + new_start));
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
                    &old[old_start..old_start + n - x],
                    &new[new_start..new_start + m - y],
                );
                x += advance;
                y += advance;
            }
            vb[k] = x;
            if !odd && (k - delta).abs() <= d && vb[k] + vf[delta - k] >= n {
                return Some((n - x + old_start, m - y + new_start));
            }
        }
    }
    None
}

fn conquer<D: DiffHandler>(
    proxy: &mut OperateProxy<D>,
    old: &[char],
    mut old_start: usize,
    mut old_end: usize,
    new: &[char],
    mut new_start: usize,
    mut new_end: usize,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) -> () {
    let common_prefix_len = common_prefix(&old[old_start..old_end], &new[new_start..new_end]);
    if common_prefix_len > 0 {
        proxy.flush_del_ins();
        old_start += common_prefix_len;
        new_start += common_prefix_len;
    }

    let common_suffix_len = common_suffix_len(&old[old_start..old_end], &new[new_start..new_end]);
    old_end -= common_suffix_len;
    new_end -= common_suffix_len;
    if is_not_empty_range(old_start, old_end) || is_not_empty_range(new_start, new_end) {
        if is_empty_range(new_start, new_end) {
            proxy.delete(old_start, old_end - old_start);
        } else if is_empty_range(old_start, old_end) {
            proxy.insert(old_start, new_start, new_end - new_start);
        } else if let Some((x_start, y_start)) =
            find_middle_snake(old, old_start, old_end, new, new_start, new_end, vf, vb)
        {
            conquer(
                proxy, old, old_start, x_start, new, new_start, y_start, vf, vb,
            );
            conquer(proxy, old, x_start, old_end, new, y_start, new_end, vf, vb);
        } else {
            proxy.delete(old_start, old_end - old_start);
            proxy.insert(old_start, new_start, new_end - new_start);
        }
    }

    if common_suffix_len > 0 {
        proxy.flush_del_ins();
    }
}
