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
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::iter::zip;
use std::ops::{Index, IndexMut};

use fxhash::FxHashMap;

/// Utility function to check if a range is empty that works on older rust versions
#[inline(always)]
fn is_empty_range(start: usize, end: usize) -> bool {
    start >= end
}

#[inline(always)]
fn is_not_empty_range(start: usize, end: usize) -> bool {
    start < end
}

fn common_prefix(xs: &[u32], ys: &[u32]) -> usize {
    let chunk_size = 4;
    let off = zip(xs.chunks_exact(chunk_size), ys.chunks_exact(chunk_size))
        .take_while(|(xs_chunk, ys_chunk)| xs_chunk == ys_chunk)
        .count()
        * chunk_size;
    off + zip(&xs[off..], &ys[off..])
        .take_while(|(x, y)| x == y)
        .count()
}

fn common_suffix_len(old: &[u32], new: &[u32]) -> usize {
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
}

#[derive(Debug)]
pub(crate) struct OperateProxy<D: DiffHandler> {
    handler: D,
}

impl<D: DiffHandler> OperateProxy<D> {
    pub fn new(handler: D) -> Self {
        Self { handler }
    }

    pub fn delete(&mut self, old_index: usize, old_len: usize) {
        self.handler.delete(old_index, old_len);
    }

    pub fn insert(&mut self, old_index: usize, new_index: usize, new_len: usize) {
        self.handler.insert(old_index, new_index, new_len);
    }

    fn unwrap(self) -> D {
        self.handler
    }
}

pub(crate) fn myers_diff<D: DiffHandler>(proxy: &mut OperateProxy<D>, old: &[u32], new: &[u32]) {
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

#[allow(clippy::too_many_arguments)]
fn find_middle_snake(
    old: &[u32],
    old_start: usize,
    old_end: usize,
    new: &[u32],
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

#[allow(clippy::too_many_arguments)]
fn conquer<D: DiffHandler>(
    proxy: &mut OperateProxy<D>,
    old: &[u32],
    mut old_start: usize,
    mut old_end: usize,
    new: &[u32],
    mut new_start: usize,
    mut new_end: usize,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) {
    let common_prefix_len = common_prefix(&old[old_start..old_end], &new[new_start..new_end]);
    if common_prefix_len > 0 {
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
}

pub fn dj_diff<D: DiffHandler>(proxy: &mut OperateProxy<D>, old: &[u32], new: &[u32]) {
    if old.is_empty() {
        proxy.insert(0, 0, new.len());
        return;
    }

    if new.is_empty() {
        proxy.delete(0, old.len());
        return;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Point {
        x: u32,
        y: u32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Direction {
        Up,
        Left,
        UpLeft,
    }

    struct QueueItem {
        point: Point,
        cost: isize,
        from: Direction,
    }

    impl PartialEq for QueueItem {
        fn eq(&self, other: &Self) -> bool {
            self.cost == other.cost
        }
    }

    impl PartialOrd for QueueItem {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(&other))
        }
    }

    impl Ord for QueueItem {
        fn cmp(&self, other: &Self) -> Ordering {
            other.cost.cmp(&self.cost)
        }
    }

    impl Eq for QueueItem {}

    let mut visited: FxHashMap<Point, Direction> = FxHashMap::default();
    let mut q: BinaryHeap<QueueItem> = BinaryHeap::new();
    q.push(QueueItem {
        point: Point { x: 0, y: 0 },
        cost: 0,
        from: Direction::UpLeft,
    });

    while let Some(QueueItem { point, cost, from }) = q.pop() {
        // dbg!(&point, &cost, &from);
        if point.x == old.len() as u32 && point.y == new.len() as u32 {
            break;
        }

        if point.x + 1 <= old.len() as u32 {
            let next_point = Point {
                x: point.x + 1,
                y: point.y,
            };

            if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(next_point) {
                e.insert(Direction::Left);
                let mut next_cost = cost + 1;
                if from != Direction::Left {
                    next_cost += 1;
                }

                q.push(QueueItem {
                    point: next_point,
                    cost: next_cost,
                    from: Direction::Left,
                });
            }
        }

        if point.y + 1 <= new.len() as u32 {
            let next_point = Point {
                x: point.x,
                y: point.y + 1,
            };

            if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(next_point) {
                let direction = Direction::Up;
                let mut next_cost = cost + 1;
                if from != direction {
                    next_cost += 1;
                }

                e.insert(direction);
                q.push(QueueItem {
                    point: next_point,
                    cost: next_cost,
                    from: Direction::Up,
                });
            }
        }

        if point.x + 1 <= old.len() as u32
            && point.y + 1 <= new.len() as u32
            && old[point.x as usize] == new[point.y as usize]
        {
            let next_point = Point {
                x: point.x + 1,
                y: point.y + 1,
            };

            if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(next_point) {
                let next_cost = if from == Direction::UpLeft {
                    cost
                } else {
                    cost + 1
                };

                e.insert(Direction::UpLeft);
                q.push(QueueItem {
                    point: next_point,
                    cost: next_cost,
                    from: Direction::UpLeft,
                });
            }
        }
    }

    // Backtrack from end point to construct diff operations
    let mut current = Point {
        x: old.len() as u32,
        y: new.len() as u32,
    };

    let mut path: Vec<(Direction, usize)> = Vec::new();
    while current.x > 0 || current.y > 0 {
        let direction = visited.get(&current).unwrap();
        if let Some((last_dir, count)) = path.last_mut() {
            if last_dir == direction {
                *count += 1;
            } else {
                path.push((*direction, 1));
            }
        } else {
            path.push((*direction, 1));
        }
        match direction {
            Direction::Left => {
                current.x -= 1;
            }
            Direction::Up => {
                current.y -= 1;
            }
            Direction::UpLeft => {
                current.x -= 1;
                current.y -= 1;
            }
        }
    }

    path.reverse();

    let mut old_index = 0;
    let mut new_index = 0;

    for (direction, count) in path {
        match direction {
            Direction::Left => {
                proxy.delete(old_index, count);
                old_index += count;
            }
            Direction::Up => {
                proxy.insert(old_index, new_index, count);
                new_index += count;
            }
            Direction::UpLeft => {
                old_index += count;
                new_index += count;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct RecordingDiffHandler {
        ops: Vec<DiffOperation>,
    }

    #[derive(Debug, PartialEq)]
    enum DiffOperation {
        Insert {
            old_index: usize,
            new_index: usize,
            length: usize,
        },
        Delete {
            old_index: usize,
            length: usize,
        },
    }

    impl DiffHandler for RecordingDiffHandler {
        fn insert(&mut self, pos: usize, start: usize, len: usize) {
            self.ops.push(DiffOperation::Insert {
                old_index: pos,
                new_index: start,
                length: len,
            });
        }

        fn delete(&mut self, pos: usize, len: usize) {
            self.ops.push(DiffOperation::Delete {
                old_index: pos,
                length: len,
            });
        }
    }

    #[test]
    fn test_recording_diff_handler() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3];
        let new = vec![1, 4, 3];

        dj_diff(&mut proxy, &old, &new);
        let handler = proxy.unwrap();
        assert_eq!(
            handler.ops,
            vec![
                DiffOperation::Delete {
                    old_index: 1,
                    length: 1
                },
                DiffOperation::Insert {
                    old_index: 2,
                    new_index: 1,
                    length: 1
                },
            ]
        );
    }

    #[test]
    fn test_dj_diff_same() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3];
        let new = vec![1, 2, 3];
        dj_diff(&mut proxy, &old, &new);
        let handler = proxy.unwrap();
        assert_eq!(handler.ops, vec![]);
    }

    #[test]
    fn test_dj_diff_1() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1];
        let new = vec![0, 1, 2];
        dj_diff(&mut proxy, &old, &new);
        let handler = proxy.unwrap();
        assert_eq!(
            handler.ops,
            vec![
                DiffOperation::Insert {
                    old_index: 0,
                    new_index: 0,
                    length: 1
                },
                DiffOperation::Insert {
                    old_index: 1,
                    new_index: 2,
                    length: 1
                },
            ]
        );
    }

    #[test]
    fn test_dj_diff_may_scatter() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3, 4, 5];
        let new = vec![99, 1, 2, 3, 4, 5, 98, 97, 96, 3, 95, 4, 93, 92, 5, 91];
        dj_diff(&mut proxy, &old, &new);
        let handler = proxy.unwrap();
        assert_eq!(
            handler.ops,
            vec![
                DiffOperation::Insert {
                    old_index: 0,
                    new_index: 0,
                    length: 1
                },
                DiffOperation::Insert {
                    old_index: 5,
                    new_index: 6,
                    length: 10
                },
            ]
        );
    }

    #[test]
    fn test_dj_diff_may_scatter_1() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3, 4, 5];
        let new = vec![99, 1, 2, 98, 97, 96, 3, 95, 4, 93, 92, 5, 1, 2, 3, 4, 5, 91];
        dj_diff(&mut proxy, &old, &new);
        let handler = proxy.unwrap();
        assert_eq!(
            handler.ops,
            vec![
                DiffOperation::Insert {
                    old_index: 0,
                    new_index: 0,
                    length: 12
                },
                DiffOperation::Insert {
                    old_index: 5,
                    new_index: 17,
                    length: 1
                },
            ]
        );
    }
}
