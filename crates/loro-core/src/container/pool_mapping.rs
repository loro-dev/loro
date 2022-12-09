use fxhash::FxHashMap;
use rle::range_map::RangeMap;
use std::{fmt::Debug, ops::Range};

use crate::{container::text::text_content::SliceRange, InternalString, LoroValue};

use super::map::ValueSlot;

#[derive(Debug, Default)]
pub struct PoolMapping<T> {
    new: Vec<T>,
    old2new: RangeMap<u32, Range<u32>>,
    pub(crate) new_state_len: u32,
}

pub enum StateContent {
    List {
        pool: Vec<LoroValue>,
        state_len: u32,
    },
    Map {
        pool: Vec<LoroValue>,
        keys: Vec<InternalString>,
        values: Vec<ValueSlot>,
    },
    Text {
        pool: Vec<u8>,
        state_len: u32,
        utf_16: i32,
    },
}

impl<T: Clone + Debug> PoolMapping<T> {
    pub fn push_state_slice(&mut self, old_slice: Range<u32>, old: &[T]) -> Range<u32> {
        let start_index = self.new.len() as u32;
        for v in old[old_slice.start as usize..old_slice.end as usize].iter() {
            self.new.push(v.clone());
        }
        let end_index = self.new.len() as u32;
        self.old2new
            .set_small_range(old_slice.start, start_index..end_index);
        start_index..end_index
    }

    pub fn push_state_slice_finish(&mut self) {
        self.new_state_len = self.new.len() as u32;
    }

    pub fn convert_ops_slice(
        &mut self,
        old_slice: Range<u32>,
        old_pool: Option<&[T]>,
    ) -> Vec<SliceRange> {
        let range_sliced = self
            .old2new
            .get_range_sliced(old_slice.start, old_slice.end)
            .collect::<Vec<_>>();
        let mut ans = Vec::new();
        let mut cursor = old_slice.start;
        for (old_index, new_range) in range_sliced {
            if old_index != cursor {
                // missing
                self.add_missing(cursor, old_index, old_pool, &mut ans);
            }
            // covered
            cursor = old_index + (new_range.end - new_range.start);
            ans.push(SliceRange::from(new_range));
        }
        if cursor != old_slice.end {
            // missing
            self.add_missing(cursor, old_slice.end, old_pool, &mut ans);
        }
        ans
    }

    #[inline]
    fn add_missing(
        &mut self,
        cursor: u32,
        old_index: u32,
        old_pool: Option<&[T]>,
        ans: &mut Vec<SliceRange>,
    ) {
        let miss_len = old_index - cursor;
        if let Some(old_pool) = old_pool {
            let this_range = self.push_state_slice(cursor..cursor + miss_len, old_pool);
            ans.push(SliceRange::from(this_range));
        } else {
            ans.push(SliceRange::new_unknown(miss_len));
        }
    }

    #[inline]
    pub fn inner(self) -> Vec<T> {
        self.new
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.new.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.new.is_empty()
    }
}

#[derive(Debug)]
pub struct MapPoolMapping {
    new: Vec<LoroValue>,
    old2new: FxHashMap<u32, u32>,
}

impl Default for MapPoolMapping {
    fn default() -> Self {
        Self {
            new: vec![LoroValue::Null],
            old2new: FxHashMap::default(),
        }
    }
}

impl MapPoolMapping {
    /// LoroValue::Null is always at index 0
    pub fn push_state_slice(&mut self, old_index: u32, old: &LoroValue) -> u32 {
        if let LoroValue::Null = old {
            self.old2new.insert(old_index, 0);
            return 0;
        }
        let new_index = self.len() as u32;
        self.new.push(old.clone());
        self.old2new.insert(old_index, new_index);
        new_index
    }

    #[inline]
    pub fn convert_ops_value(&mut self, old_index: u32, old_value: &LoroValue) -> u32 {
        if let Some(new_index) = self.old2new.get(&old_index) {
            *new_index
        } else {
            self.push_state_slice(old_index, old_value)
        }
    }

    #[inline]
    pub fn get_new_index(&self, old_index: u32) -> u32 {
        *self.old2new.get(&old_index).unwrap()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.new.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.new.is_empty()
    }

    #[inline]
    pub fn inner(self) -> Vec<LoroValue> {
        self.new
    }
}
mod test {
    use crate::container::text::text_content::SliceRange;

    use super::PoolMapping;

    #[test]
    fn mapping() {
        let old_pool = vec![7, 8, 9, 6, 5];
        let old_state = vec![0..1, 3..4, 2..3];

        // let new_state = vec![7,6,9];

        let mut mapping = PoolMapping::default();

        for old_slice in old_state.into_iter() {
            mapping.push_state_slice(old_slice, &old_pool);
        }

        let new_ops = mapping.convert_ops_slice(0..3, None);
        println!("ops {:?}", new_ops);
        let new_ops2 = mapping.convert_ops_slice(3..5, None);
        println!("ops {:?}", new_ops2);
        assert_eq!(
            new_ops,
            vec![(0..1).into(), SliceRange::new_unknown(1), (2..3).into()]
        );
    }
}
