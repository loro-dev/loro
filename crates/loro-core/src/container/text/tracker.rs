use rle::HasLength;

use crate::{
    container::{list::list_op::ListOp, text::tracker::yata::YataImpl},
    id::{Counter, ID},
    op::Op,
    span::IdSpan,
    VersionVector,
};

use self::{
    content_map::ContentMap,
    cursor_map::{make_notify, CursorMap, IdSpanQueryResult},
    y_span::{Status, StatusChange, YSpan},
};

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
    /// all applied operations version vector
    all_vv: VersionVector,
    /// current content version vector
    head_vv: VersionVector,
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
            head_vv: Default::default(),
            all_vv: Default::default(),
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

    fn checkout(&mut self, vv: VersionVector) {
        let diff = self.head_vv.diff(&vv);
        self.retreat(&diff.get_id_spans_left().collect::<Vec<_>>());
        self.forward(&diff.get_id_spans_right().collect::<Vec<_>>());
        self.head_vv = vv;
    }

    fn forward(&mut self, spans: &[IdSpan]) {
        let mut to_set_as_applied = Vec::with_capacity(spans.len());
        let mut to_delete = Vec::with_capacity(spans.len());
        for span in spans.iter() {
            let IdSpanQueryResult {
                mut inserts,
                deletes,
            } = self.id_to_cursor.get_cursor_at_id_span(*span);
            for delete in deletes {
                for deleted_span in delete.iter() {
                    to_delete.append(
                        &mut self
                            .id_to_cursor
                            .get_cursor_at_id_span(*deleted_span)
                            .inserts,
                    );
                }
            }

            to_set_as_applied.append(&mut inserts);
        }

        self.content.update_at_cursors_twice(
            &[&to_set_as_applied, &to_delete],
            &mut |v| {
                v.status.apply(StatusChange::SetAsFuture);
            },
            &mut |v: &mut YSpan| {
                v.status.apply(StatusChange::UndoDelete);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    fn retreat(&mut self, spans: &[IdSpan]) {
        let mut to_set_as_future = Vec::with_capacity(spans.len());
        let mut to_undo_delete = Vec::with_capacity(spans.len());
        for span in spans.iter() {
            let IdSpanQueryResult {
                mut inserts,
                deletes,
            } = self.id_to_cursor.get_cursor_at_id_span(*span);
            for delete in deletes {
                for deleted_span in delete.iter() {
                    to_undo_delete.append(
                        &mut self
                            .id_to_cursor
                            .get_cursor_at_id_span(*deleted_span)
                            .inserts,
                    );
                }
            }

            to_set_as_future.append(&mut inserts);
        }

        self.content.update_at_cursors_twice(
            &[&to_set_as_future, &to_undo_delete],
            &mut |v| {
                v.status.apply(StatusChange::SetAsFuture);
            },
            &mut |v: &mut YSpan| {
                v.status.apply(StatusChange::UndoDelete);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    /// apply an operation directly to the current tracker
    fn apply(&mut self, op: &Op) {
        if self.all_vv.includes_id(op.id.inc(op.len() as i32 - 1)) {}
        assert_eq!(
            *self.head_vv.get(&op.id.client_id).unwrap_or(&0),
            op.id.counter
        );
        self.head_vv.set_end(op.id.inc(op.len() as i32));
        let id = op.id;
        match &op.content {
            crate::op::OpContent::Normal { content } => {
                let text_content = content.as_list().expect("Content is not for list");
                match text_content {
                    ListOp::Insert { slice, pos } => {
                        let yspan = self.content.get_yspan_at_pos(id, *pos, slice.len());
                        // SAFETY: we know this is safe because in [YataImpl::insert_after] there is no access to shared elements
                        unsafe { crdt_list::yata::integrate::<YataImpl>(self, yspan) };
                    }
                    ListOp::Delete { pos, len } => {
                        let spans = self.content.get_id_spans(*pos, *len);
                        self.update_spans(&spans, StatusChange::Delete);
                        self.id_to_cursor
                            .set((id).into(), cursor_map::Marker::Delete(spans));
                    }
                }
            }
            crate::op::OpContent::Undo { .. } => todo!(),
            crate::op::OpContent::Redo { .. } => todo!(),
        }
    }

    fn update_spans(&mut self, spans: &[IdSpan], change: StatusChange) {
        let mut cursors = Vec::new();
        for span in spans.iter() {
            let mut inserts = self.id_to_cursor.get_cursor_at_id_span(*span).inserts;
            cursors.append(&mut inserts);
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
