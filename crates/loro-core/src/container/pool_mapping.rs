// use num::iter::Range;
// use rle::range_map::RangeMap;
// use smallvec::{smallvec, SmallVec};

// pub struct PoolMapping<'a, T> {
//     new: Vec<T>,
//     mapping_from_new_to_old: RangeMap<usize, Range<usize>>,
//     mapping_from_old_to_new: RangeMap<usize, Range<usize>>,
// }

// impl<'a, T: Clone> PoolMapping<'a, T> {
//     pub fn push_slice(
//         &mut self,
//         old_slice: Range<usize>,
//         old: &[T],
//     ) -> SmallVec<[Range<usize>; 1]> {
//         // ab
//         // b
//         // [b]  1..2
//         // ab - 0..2

//         let start_index = self.new.len();
//         for v in old[old_slice] {
//             self.new.push(v.clone());
//         }
//         let end_index = self.new.len();
//         self.mapping_from_new_to_old
//             .set_small_range(start_index, old_slice);
//         self.mapping_from_old_to_new
//             .set_small_range(old_slice.start, start_index..end_index);

//         smallvec![start_index..end_index]
//     }
// }
