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
use crate::change::get_sys_timestamp;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::iter::zip;
use std::ops::{Index, IndexMut};

/// Options for controlling the text update behavior.
///
/// - `timeout_ms`: Optional timeout in milliseconds for the diff computation
/// - `use_refined_diff`: Whether to use a more refined but slower diff algorithm. Defaults to true.
#[derive(Clone, Debug)]
pub struct UpdateOptions {
    pub timeout_ms: Option<f64>,
    pub use_refined_diff: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            timeout_ms: None,
            use_refined_diff: true,
        }
    }
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum UpdateTimeoutError {
    #[error("Timeout")]
    Timeout,
}

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

    #[allow(unused)]
    fn unwrap(self) -> D {
        self.handler
    }
}

pub(crate) fn diff<D: DiffHandler>(
    proxy: &mut OperateProxy<D>,
    options: UpdateOptions,
    old: &[u32],
    new: &[u32],
) -> Result<(), UpdateTimeoutError> {
    let max_d = (old.len() + new.len()).div_ceil(2) + 1;
    let mut vb = OffsetVec::new(max_d);
    let mut vf = OffsetVec::new(max_d);
    let start_time = if options.timeout_ms.is_some() {
        get_sys_timestamp()
    } else {
        0.
    };

    conquer(
        proxy,
        options.use_refined_diff,
        options.timeout_ms,
        start_time,
        old,
        0,
        old.len(),
        new,
        0,
        new.len(),
        &mut vf,
        &mut vb,
    )
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

struct MiddleSnakeResult {
    #[allow(unused)]
    d: usize,
    x_start: usize,
    y_start: usize,
}

#[allow(clippy::too_many_arguments)]
fn find_middle_snake(
    timeout_ms: Option<f64>,
    start_time: f64,
    old: &[u32],
    old_start: usize,
    old_end: usize,
    new: &[u32],
    new_start: usize,
    new_end: usize,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) -> Result<Option<MiddleSnakeResult>, UpdateTimeoutError> {
    let n = old_end - old_start;
    let m = new_end - new_start;
    let delta = n as isize - m as isize;
    let odd = delta & 1 != 0;
    vf[1] = 0;
    vb[1] = 0;
    let d_max = (n + m).div_ceil(2) + 1;
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
                return Ok(Some(MiddleSnakeResult {
                    d: d as usize,
                    x_start: x0 + old_start,
                    y_start: y0 + new_start,
                }));
            }

            if let Some(timeout_ms) = timeout_ms {
                if get_sys_timestamp() - start_time > timeout_ms {
                    return Err(UpdateTimeoutError::Timeout);
                }
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
                return Ok(Some(MiddleSnakeResult {
                    d: d as usize,
                    x_start: n - x + old_start,
                    y_start: m - y + new_start,
                }));
            }

            if let Some(timeout_ms) = timeout_ms {
                if get_sys_timestamp() - start_time > timeout_ms {
                    return Err(UpdateTimeoutError::Timeout);
                }
            }
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn conquer<D: DiffHandler>(
    proxy: &mut OperateProxy<D>,
    should_use_dj: bool,
    timeout_ms: Option<f64>,
    start_time: f64,
    old: &[u32],
    mut old_start: usize,
    mut old_end: usize,
    new: &[u32],
    mut new_start: usize,
    mut new_end: usize,
    vf: &mut OffsetVec,
    vb: &mut OffsetVec,
) -> Result<(), UpdateTimeoutError> {
    let common_prefix_len = common_prefix(&old[old_start..old_end], &new[new_start..new_end]);
    if common_prefix_len > 0 {
        old_start += common_prefix_len;
        new_start += common_prefix_len;
    }

    let common_suffix_len = common_suffix_len(&old[old_start..old_end], &new[new_start..new_end]);
    old_end -= common_suffix_len;
    new_end -= common_suffix_len;

    if is_not_empty_range(old_start, old_end) || is_not_empty_range(new_start, new_end) {
        let len_old = old_end - old_start;
        let len_new = new_end - new_start;
        if should_use_dj && (len_old.max(1) * len_new.max(1) < 128 * 128) {
            let ok = dj_diff(
                proxy,
                &old[old_start..old_end],
                &new[new_start..new_end],
                old_start,
                new_start,
                10_000,
            );
            if ok {
                return Ok(());
            }
        }

        if is_empty_range(new_start, new_end) {
            proxy.delete(old_start, old_end - old_start);
        } else if is_empty_range(old_start, old_end) {
            proxy.insert(old_start, new_start, new_end - new_start);
        } else if let Some(MiddleSnakeResult {
            d: _,
            x_start,
            y_start,
        }) = find_middle_snake(
            timeout_ms, start_time, old, old_start, old_end, new, new_start, new_end, vf, vb,
        )? {
            conquer(
                proxy,
                should_use_dj,
                timeout_ms,
                start_time,
                old,
                old_start,
                x_start,
                new,
                new_start,
                y_start,
                vf,
                vb,
            )?;
            conquer(
                proxy,
                should_use_dj,
                timeout_ms,
                start_time,
                old,
                x_start,
                old_end,
                new,
                y_start,
                new_end,
                vf,
                vb,
            )?;
        } else {
            proxy.delete(old_start, old_end - old_start);
            proxy.insert(old_start, new_start, new_end - new_start);
        }
    }

    Ok(())
}

/// Return false if this method gives up early
#[must_use]
pub(crate) fn dj_diff<D: DiffHandler>(
    proxy: &mut OperateProxy<D>,
    old: &[u32],
    new: &[u32],
    old_offset: usize,
    new_offset: usize,
    max_try_count: usize,
) -> bool {
    let common_prefix_len = common_prefix(old, new);
    let common_suffix_len = common_suffix_len(&old[common_prefix_len..], &new[common_prefix_len..]);
    let old = &old[common_prefix_len..old.len() - common_suffix_len];
    let new = &new[common_prefix_len..new.len() - common_suffix_len];
    if old.len() >= u16::MAX as usize || new.len() >= u16::MAX as usize {
        return false;
    }

    if old.is_empty() {
        if new.is_empty() {
            return true;
        }

        proxy.insert(
            old_offset + common_prefix_len,
            new_offset + common_prefix_len,
            new.len(),
        );
        return true;
    }

    if new.is_empty() {
        proxy.delete(old_offset + common_prefix_len, old.len());
        return true;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Point {
        x: u16,
        y: u16,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Direction {
        Up,
        Left,
        UpLeft,
    }

    struct QueueItem {
        point: Point,
        cost: u32,
        from: Direction,
    }

    impl PartialEq for QueueItem {
        fn eq(&self, other: &Self) -> bool {
            self.cost == other.cost
        }
    }

    impl PartialOrd for QueueItem {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
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
        if visited.contains_key(&point) {
            continue;
        }

        visited.insert(point, from);
        if point.x == old.len() as u16 && point.y == new.len() as u16 {
            break;
        }

        if visited.len() + q.len() > max_try_count {
            // println!("give up on: visited len: {}", visited.len());
            // println!("queue len: {}", q.len());
            return false;
        }

        if point.x < old.len() as u16 {
            let next_point = Point {
                x: point.x + 1,
                y: point.y,
            };

            if !visited.contains_key(&next_point) {
                let mut next_cost = cost + 1;
                if from != Direction::Left {
                    next_cost += 8;
                }

                q.push(QueueItem {
                    point: next_point,
                    cost: next_cost,
                    from: Direction::Left,
                });
            }
        }

        if point.y < new.len() as u16 {
            let next_point = Point {
                x: point.x,
                y: point.y + 1,
            };

            if !visited.contains_key(&next_point) {
                let direction = Direction::Up;
                let mut next_cost = cost + 1;
                if from != direction {
                    next_cost += 8;
                }

                q.push(QueueItem {
                    point: next_point,
                    cost: next_cost,
                    from: Direction::Up,
                });
            }
        }

        if point.x < old.len() as u16
            && point.y < new.len() as u16
            && old[point.x as usize] == new[point.y as usize]
        {
            let next_point = Point {
                x: point.x + 1,
                y: point.y + 1,
            };

            if !visited.contains_key(&next_point) {
                let next_cost = if from == Direction::UpLeft {
                    cost
                } else {
                    cost + 1
                };

                q.push(QueueItem {
                    point: next_point,
                    cost: next_cost,
                    from: Direction::UpLeft,
                });
            }
        }
    }

    // println!("visited len: {}", visited.len());
    // println!("queue len: {}", q.len());

    // Backtrack from end point to construct diff operations
    let mut current = Point {
        x: old.len() as u16,
        y: new.len() as u16,
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

    let mut old_index = common_prefix_len;
    let mut new_index = common_prefix_len;

    for (direction, count) in path {
        match direction {
            Direction::Left => {
                proxy.delete(old_offset + old_index, count);
                old_index += count;
            }
            Direction::Up => {
                proxy.insert(old_offset + old_index, new_index + new_offset, count);
                new_index += count;
            }
            Direction::UpLeft => {
                old_index += count;
                new_index += count;
            }
        }
    }

    true
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

        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
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
        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
        let handler = proxy.unwrap();
        assert_eq!(handler.ops, vec![]);
    }

    #[test]
    fn test_dj_diff_1() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1];
        let new = vec![0, 1, 2];
        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
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
    fn test_diff_may_scatter() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3, 4, 5];
        let new = vec![99, 1, 2, 3, 4, 5, 98, 97, 96, 3, 95, 4, 93, 92, 5, 91];
        diff(&mut proxy, UpdateOptions::default(), &old, &new).unwrap();
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
    fn test_diff_may_scatter_1() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3, 4, 5];
        let new = vec![99, 1, 2, 98, 97, 96, 3, 95, 4, 93, 92, 5, 1, 2, 3, 4, 5, 91];
        diff(&mut proxy, UpdateOptions::default(), &old, &new).unwrap();
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

    #[test]
    fn test_dj_diff_may_scatter() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1, 2, 3, 4, 5];
        let new = vec![99, 1, 2, 3, 4, 5, 98, 97, 96, 3, 95, 4, 93, 92, 5, 91];
        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
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
        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
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

    #[test]
    fn test_dj_diff_100_differences() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1; 100];
        let new = vec![2; 100];
        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
        let handler = proxy.unwrap();
        assert_eq!(
            handler.ops,
            vec![
                DiffOperation::Delete {
                    old_index: 0,
                    length: 100
                },
                DiffOperation::Insert {
                    old_index: 100,
                    new_index: 0,
                    length: 100
                },
            ]
        );
    }

    #[test]
    fn test_dj_diff_insert() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1; 100];
        let mut new = old.clone();
        new.splice(50..50, [1, 2, 3, 4, 1, 2, 3, 4]);
        new.splice(0..0, [0, 1, 2, 3, 4, 5, 6, 7]);
        let _ = dj_diff(&mut proxy, &old, &new, 0, 0, 100_000);
        let handler = proxy.unwrap();
        assert_eq!(
            handler.ops,
            vec![
                DiffOperation::Insert {
                    old_index: 0,
                    new_index: 0,
                    length: 9
                },
                DiffOperation::Insert {
                    old_index: 50,
                    new_index: 59,
                    length: 7
                }
            ]
        );
    }

    #[test]
    fn test_diff() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1; 100];
        let new = vec![2; 100];
        diff(&mut proxy, Default::default(), &old, &new).unwrap();
    }

    #[test]
    fn test_timeout() {
        let handler = RecordingDiffHandler::default();
        let mut proxy = OperateProxy::new(handler);
        let old = vec![1; 10000];
        let new = vec![2; 10000];
        let options = UpdateOptions {
            timeout_ms: Some(0.1),
            ..Default::default()
        };
        let result = diff(&mut proxy, options, &old, &new);
        assert!(result.is_err());
    }
}
