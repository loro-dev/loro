use rle::{rle_tree::UnsafeCursor, HasLength};
use smallvec::SmallVec;

use crate::{
    container::{list::list_op::ListOp, text::tracker::yata_impl::YataImpl},
    id::{Counter, ID},
    op::OpContent,
    span::IdSpan,
    version::IdSpanVector,
    VersionVector,
};

use self::{
    content_map::ContentMap,
    cursor_map::{make_notify, CursorMap, IdSpanQueryResult},
    effects_iter::EffectIter,
    y_span::{Status, StatusChange, YSpan, YSpanTreeTrait},
};

pub(crate) use effects_iter::Effect;
mod content_map;
mod cursor_map;
mod effects_iter;
mod y_span;
#[cfg(not(feature = "fuzzing"))]
mod yata_impl;
#[cfg(feature = "fuzzing")]
pub mod yata_impl;

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
    /// from start_vv to latest vv are applied
    start_vv: VersionVector,
    /// latest applied ops version vector
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
    pub fn new(start_vv: VersionVector) -> Self {
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
                slice: Default::default(),
            },
            &mut make_notify(&mut id_to_cursor),
        );

        Tracker {
            content,
            id_to_cursor,
            start_vv,
            #[cfg(feature = "fuzzing")]
            client_id: 0,
            head_vv: Default::default(),
            all_vv: Default::default(),
        }
    }

    #[inline]
    pub fn start_vv(&self) -> &VersionVector {
        &self.start_vv
    }

    #[inline]
    pub fn head_vv(&self) -> &VersionVector {
        &self.head_vv
    }

    pub fn contains(&self, id: ID) -> bool {
        !self.start_vv.includes_id(id) && self.all_vv.includes_id(id)
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

    pub fn checkout(&mut self, vv: &VersionVector) {
        let diff = self.head_vv.diff(vv);
        self.retreat(&diff.left);
        self.forward(&diff.right);
        self.head_vv = vv.clone();
    }

    pub fn forward(&mut self, spans: &IdSpanVector) {
        let mut to_set_as_applied = Vec::with_capacity(spans.len());
        let mut to_delete = Vec::with_capacity(spans.len());
        for span in spans.iter() {
            self.head_vv.set_end(ID::new(*span.0, span.1.end));
            let IdSpanQueryResult { inserts, deletes } = self
                .id_to_cursor
                .get_cursors_at_id_span(IdSpan::new(*span.0, span.1.start, span.1.end));
            for (_, delete) in deletes {
                for deleted_span in delete.iter() {
                    to_delete.append(
                        &mut self
                            .id_to_cursor
                            .get_cursors_at_id_span(*deleted_span)
                            .inserts
                            .iter()
                            .map(|x| x.1)
                            .collect(),
                    );
                }
            }

            // TODO: maybe we can skip this collect
            to_set_as_applied.append(&mut inserts.iter().map(|x| x.1).collect());
        }

        self.content.update_at_cursors_twice(
            &[&to_set_as_applied, &to_delete],
            &mut |v| {
                v.status.apply(StatusChange::SetAsCurrent);
            },
            &mut |v: &mut YSpan| {
                v.status.apply(StatusChange::Delete);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    pub fn retreat(&mut self, spans: &IdSpanVector) {
        let mut to_set_as_future = Vec::with_capacity(spans.len());
        let mut to_undo_delete = Vec::with_capacity(spans.len());
        for span in spans.iter() {
            self.head_vv.set_end(ID::new(*span.0, span.1.start));
            let IdSpanQueryResult { inserts, deletes } = self
                .id_to_cursor
                .get_cursors_at_id_span(IdSpan::new(*span.0, span.1.start, span.1.end));
            for (_, delete) in deletes {
                for deleted_span in delete.iter() {
                    to_undo_delete.append(
                        &mut self
                            .id_to_cursor
                            .get_cursors_at_id_span(*deleted_span)
                            .inserts
                            .iter()
                            .map(|x| x.1)
                            .collect(),
                    );
                }
            }

            // TODO: maybe we can skip this collect
            to_set_as_future.append(&mut inserts.iter().map(|x| x.1).collect());
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
    pub(crate) fn apply(&mut self, id: ID, content: &OpContent) {
        assert_eq!(*self.head_vv.get(&id.client_id).unwrap_or(&0), id.counter);
        assert_eq!(*self.all_vv.get(&id.client_id).unwrap_or(&0), id.counter);
        self.head_vv.set_end(id.inc(content.len() as i32));
        self.all_vv.set_end(id.inc(content.len() as i32));
        match &content {
            crate::op::OpContent::Normal { content } => {
                let text_content = content.as_list().expect("Content is not for list");
                match text_content {
                    ListOp::Insert { slice, pos } => {
                        let yspan =
                            self.content
                                .get_yspan_at_pos(id, *pos, slice.len(), slice.clone());
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

    fn update_cursors(
        &mut self,
        mut cursors: SmallVec<[UnsafeCursor<'_, YSpan, YSpanTreeTrait>; 2]>,
        change: StatusChange,
    ) -> i32 {
        let mut changed: i32 = 0;
        self.content.update_at_cursors(
            &mut cursors,
            &mut |v| {
                let before = v.len() as i32;
                v.status.apply(change);
                let after = v.len() as i32;
                changed += after - before;
            },
            &mut make_notify(&mut self.id_to_cursor),
        );

        changed
    }

    fn update_spans(&mut self, spans: &[IdSpan], change: StatusChange) {
        let mut cursors: SmallVec<
            [UnsafeCursor<YSpan, rle::rle_tree::tree_trait::CumulateTreeTrait<YSpan, 4>>; 2],
        > = SmallVec::with_capacity(spans.len());
        for span in spans.iter() {
            let inserts = self.id_to_cursor.get_cursors_at_id_span(*span).inserts;
            // TODO: maybe we can skip this collect
            for x in inserts.iter() {
                cursors.push(x.1);
            }
        }

        self.content.update_at_cursors(
            &mut cursors,
            &mut |v| {
                v.status.apply(change);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    pub fn iter_effects(&mut self, target: IdSpanVector) -> EffectIter<'_> {
        EffectIter::new(self, target)
    }

    pub fn check(&mut self) {
        self.check_consistency();
    }
}
