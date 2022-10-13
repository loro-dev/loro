
use rle::{HasLength, RleVec};

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
    y_span::{Status, StatusChange, YSpan},
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
    // #[cfg(feature = "fuzzing")]
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
        let max = ID::unknown(Counter::MAX / 2);
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
            client_id: 0,
            vv: Default::default(),
        }
    }

    /// check whether id_to_cursor correctly reflect the status of the content
    fn check_consistency(&mut self) {
        for span in self.content.iter() {
            let yspan = span.as_ref();
            let id_span = IdSpan::new(
                yspan.id.client_id,
                yspan.id.counter,
                yspan.len as Counter + yspan.id.counter,
            );
            let mut len = 0;
            for marker in self
                .id_to_cursor
                .get_range(id_span.min_id().into(), id_span.end_id().into())
            {
                for span in marker.get_spans(id_span) {
                    len += span.len;
                }
            }

            assert_eq!(len, yspan.len);
        }

        self.content.debug_check();
        self.id_to_cursor.debug_check();
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
                        TextOpContent::Delete { id, pos, len } => {
                            let spans = self.content.get_id_spans(*pos, *len);
                            self.update_spans(&spans, StatusChange::Delete);
                            self.id_to_cursor
                                .set((*id).into(), cursor_map::Marker::Delete(spans));
                        }
                    }
                }
            }
            crate::op::OpContent::Undo { .. } => todo!(),
            crate::op::OpContent::Redo { .. } => todo!(),
        }
    }

    pub fn update_spans(&mut self, spans: &RleVec<IdSpan>, change: StatusChange) {
        let mut cursors = Vec::new();
        for span in spans.iter() {
            let mut group = Vec::new();
            for marker in self
                .id_to_cursor
                .get_range(span.min_id().into(), span.end_id().into())
            {
                for cursor in marker.get_spans(*span) {
                    if !group.contains(&cursor) {
                        group.push(cursor);
                    }
                }
            }

            cursors.append(&mut group);
        }

        self.content.update_at_cursors(
            cursors,
            &mut |v| {
                v.status.apply(change);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
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
