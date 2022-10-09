use crdt_list::crdt::ListCrdt;
use rle::HasLength;

use crate::{
    container::text::tracker::yata::YataImpl,
    id::{Counter, ID},
    op::{utils::downcast_ref, Op},
    span::IdSpan,
    VersionVector,
};

use self::{
    content_map::ContentMap,
    cursor_map::{make_notify, CursorMap},
    y_span::{Status, YSpan},
};

use super::text_content::TextOpContent;

mod content_map;
mod cursor_map;
mod y_span;
#[cfg(not(feature = "fuzzing"))]
mod yata;
#[cfg(feature = "fuzzing")]
pub mod yata;

/// A tracker for a single text, we can use it to calculate the effect of an operation on a text.
///
/// # Note
///
/// - [YSpan] never gets removed in both [ContentMap] and [CursorMap]
///     - The deleted contents are marked with deleted, but still lives on the [ContentMap] with length of 0
///
#[derive(Debug)]
pub struct Tracker {
    #[cfg(feature = "fuzzing")]
    client_id: u64,
    vv: VersionVector,
    content: ContentMap,
    id_to_cursor: CursorMap,
}

impl From<ID> for u128 {
    fn from(id: ID) -> Self {
        ((id.client_id as u128) << 64) | id.counter as u128
    }
}

impl Tracker {
    pub fn new() -> Self {
        let min = ID::unknown(0);
        let max = ID::unknown(Counter::MAX);
        let len = (max.counter - min.counter) as usize;
        let mut content: ContentMap = Default::default();
        let mut id_to_cursor: CursorMap = Default::default();
        content.insert_notify(
            0,
            YSpan {
                origin_left: None,
                origin_right: None,
                id: min,
                status: Status::new(),
                len,
            },
            &mut make_notify(&mut id_to_cursor),
        );

        Tracker {
            content,
            id_to_cursor,
            #[cfg(feature = "fuzzing")]
            client_id: 0,
            vv: Default::default(),
        }
    }

    /// check whether id_to_cursor correctly reflect the status of the content
    fn check_consistency(&self) {
        todo!()
    }

    fn turn_on(&mut self, _id: IdSpan) {}
    fn turn_off(&mut self, _id: IdSpan) {}
    fn checkout(&mut self, _vv: VersionVector) {}

    /// apply an operation directly to the current tracker
    fn apply(&mut self, op: &Op) {
        assert_eq!(*self.vv.get(&op.id.client_id).unwrap_or(&0), op.id.counter);
        self.vv.set_end(op.id.inc(op.len() as i32));
        match &op.content {
            crate::op::OpContent::Normal { content } => {
                if let Some(text_content) = downcast_ref::<TextOpContent>(&**content) {
                    match text_content {
                        TextOpContent::Insert { id, text, pos } => {
                            let yspan = self.content.get_yspan_at_pos(*id, *pos, text.len());
                            // SAFETY: we know this is safe because in [YataImpl::insert_after] there is no access to shared elements
                            unsafe { crdt_list::yata::integrate::<YataImpl>(self, yspan) };
                        }
                        TextOpContent::Delete {
                            id: _,
                            pos: _,
                            len: _,
                        } => todo!(),
                    }
                }
            }
            crate::op::OpContent::Undo { .. } => todo!(),
            crate::op::OpContent::Redo { .. } => todo!(),
        }
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_turn_off() {
        let _tracker = Tracker::new();
        // tracker.turn_off(IdSpan::new(1, 1, 2));
    }
}
