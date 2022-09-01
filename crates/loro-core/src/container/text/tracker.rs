use crate::{op::Op, span::IdSpan, VersionVector};

use self::index_map::IndexMap;
use rle::rle_tree::tree_trait::GlobalIndex;

use super::text_content::TextOpContent;

mod index_map;
mod range_map;

struct Tracker {
    index: IndexMap,
}

impl Tracker {
    fn turn_on(&mut self, id: IdSpan) {}
    fn turn_off(&mut self, id: IdSpan) {}
    fn checkout(&mut self, vv: VersionVector) {}
    fn apply(&mut self, content: &Op) {}
}

#[cfg(test)]
mod test {
    use super::*;

    fn create_tracker() -> Tracker {
        Tracker {
            index: Default::default(),
        }
    }

    #[test]
    fn test_turn_off() {
        let mut tracker = create_tracker();
        tracker.turn_off(IdSpan::new(1, 1, 2));
    }
}
