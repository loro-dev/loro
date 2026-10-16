use std::{cell::Cell, ops::Range};

use rle::Sliceable;

use crate::{container::richtext::RichtextChunk, op::ValueSlice};

/// The values of the insert ops applied to a list tracker.
///
/// The tracker's chunks are ranges in this table, which holds the ops' own
/// [`ValueSlice`]s, the way [`super::tracker_texts::TrackerTexts`] holds text.
/// The table is reset with its tracker. See `context/text-arena-growth.md`.
#[derive(Debug, Default)]
pub(super) struct TrackerValues {
    /// Where each entry of `slices` starts, sorted. An entry ends where the
    /// next one starts, the last one at `len`. Kept apart from `slices` so the
    /// binary search reads a dense array.
    starts: Vec<u32>,
    slices: Vec<ValueSlice>,
    len: u32,
    /// The entry the last lookup ended in; a diff often reads nearby entries.
    hint: Cell<usize>,
}

impl TrackerValues {
    /// Records `values` and returns their range.
    pub fn push(&mut self, values: &ValueSlice) -> Range<u32> {
        let start = self.len;
        let end = u32::try_from(values.len())
            .ok()
            .and_then(|len| start.checked_add(len))
            .filter(|&end| end < RichtextChunk::MOVE_ANCHOR)
            .expect("list tracker values exceed the u32 index space");
        if start < end {
            if !self
                .slices
                .last_mut()
                .is_some_and(|last| last.extend_if_adjacent(values))
            {
                self.starts.push(start);
                self.slices.push(values.clone());
            }
            self.len = end;
        }
        start..end
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    fn entry_end(&self, i: usize) -> u32 {
        self.starts.get(i + 1).copied().unwrap_or(self.len)
    }

    /// The entry that holds `pos`.
    fn find(&self, pos: u32) -> usize {
        let hint = self.hint.get();
        for i in [hint, hint + 1] {
            if i < self.starts.len() && self.starts[i] <= pos && pos < self.entry_end(i) {
                return i;
            }
        }
        self.starts.partition_point(|start| *start <= pos) - 1
    }

    /// Calls `f(offset, values)` for the values in `range`, in order. `offset`
    /// is the offset of `values` from `range.start`.
    pub fn for_each_slice(&self, range: Range<u32>, mut f: impl FnMut(usize, ValueSlice)) {
        assert!(range.end <= self.len, "{range:?} is out of {}", self.len);
        if range.start >= range.end {
            return;
        }

        let mut i = self.find(range.start);
        let mut cur = range.start;
        loop {
            let start = self.starts[i];
            let values = &self.slices[i];
            let from = (cur - start) as usize;
            let to = (range.end.min(self.entry_end(i)) - start) as usize;
            let slice = if from == 0 && to == values.len() {
                values.clone()
            } else {
                values.slice(from, to)
            };
            f((cur - range.start) as usize, slice);
            cur = start + to as u32;
            if cur >= range.end {
                break;
            }
            i += 1;
        }
        self.hint.set(i);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::LoroValue;

    #[test]
    fn slices_across_ops() {
        let mut values = TrackerValues::default();
        let a = ValueSlice::new(vec![LoroValue::I64(1), LoroValue::I64(2)]);
        let b = ValueSlice::new((3..8).map(LoroValue::I64).collect());
        assert_eq!(values.push(&a), 0..2);
        assert_eq!(values.push(&b), 2..7);

        let mut got = Vec::new();
        values.for_each_slice(1..5, |offset, s| got.push((offset, s.as_slice().to_vec())));
        assert_eq!(
            got,
            vec![
                (0, vec![LoroValue::I64(2)]),
                (1, (3..6).map(LoroValue::I64).collect())
            ]
        );
    }
}
