use rle::{rle_tree::UnsafeCursor, HasLength};
use smallvec::SmallVec;

use crate::{
    container::{list::list_op::ListOp, text::tracker::yata_impl::YataImpl},
    debug_log,
    id::{Counter, ID},
    op::OpContent,
    span::{HasIdSpan, IdSpan},
    version::IdSpanVector,
    VersionVector,
};

#[allow(unused)]
use crate::ClientID;

use self::{
    content_map::ContentMap,
    cursor_map::{make_notify, CursorMap, IdSpanQueryResult},
    effects_iter::EffectIter,
    y_span::{Status, StatusChange, YSpan, YSpanTreeTrait},
};

pub(crate) use effects_iter::Effect;

use super::text_content::ListSlice;
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
    client_id: ClientID,
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
    pub fn new(start_vv: VersionVector, init_len: Counter) -> Self {
        let mut content: ContentMap = Default::default();
        let mut id_to_cursor: CursorMap = Default::default();
        if init_len > 0 {
            content.insert_notify(
                0,
                YSpan {
                    origin_left: None,
                    origin_right: None,
                    id: ID::unknown(0),
                    status: Status::new(),
                    slice: ListSlice::unknown_range(init_len as usize),
                },
                &mut make_notify(&mut id_to_cursor),
            );
        }
        Tracker {
            content,
            id_to_cursor,
            #[cfg(feature = "fuzzing")]
            client_id: 0,
            head_vv: start_vv.clone(),
            all_vv: start_vv.clone(),
            start_vv,
        }
    }

    #[inline]
    pub fn start_vv(&self) -> &VersionVector {
        &self.start_vv
    }

    pub fn all_vv(&self) -> &VersionVector {
        &self.all_vv
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
                yspan.atom_len() as Counter + yspan.id.counter,
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

            assert_eq!(len, yspan.atom_len());
        }

        self.content.debug_check();
        self.id_to_cursor.debug_check();
    }

    pub fn checkout(&mut self, vv: VersionVector) {
        let diff = self.head_vv.diff(&vv);
        self.retreat(&diff.left);
        self.forward(&diff.right);
        debug_assert_eq!(self.head_vv, vv);
    }

    pub fn checkout_to_latest(&mut self) {
        let diff = self.head_vv.diff(&self.all_vv);
        self.forward(&diff.right);
        debug_assert_eq!(self.head_vv, self.all_vv);
    }

    pub fn forward(&mut self, spans: &IdSpanVector) {
        if spans.is_empty() {
            return;
        }

        let mut cursors = Vec::with_capacity(spans.len());
        let mut args = Vec::with_capacity(spans.len());
        for span in spans.iter() {
            self.head_vv.set_end(ID::new(*span.0, span.1.end));
            if let Some(all_end_ctr) = self.all_vv.get_mut(span.0) {
                if *all_end_ctr < span.1.end {
                    // there may be holes when there are multiple containers
                    *all_end_ctr = span.1.end;
                }

                if *all_end_ctr <= span.1.start {
                    continue;
                }
            }

            let IdSpanQueryResult { inserts, deletes } = self
                .id_to_cursor
                .get_cursors_at_id_span(IdSpan::new(*span.0, span.1.start, span.1.end));
            for (_, delete) in deletes {
                for deleted_span in delete.iter() {
                    for span in self
                        .id_to_cursor
                        .get_cursors_at_id_span(*deleted_span)
                        .inserts
                        .into_iter()
                        .map(|x| x.1)
                    {
                        cursors.push(span);
                        args.push(StatusChange::Delete);
                    }
                }
            }

            for span in inserts.into_iter().map(|x| x.1) {
                cursors.push(span);
                args.push(StatusChange::SetAsCurrent);
            }
        }

        self.content.update_at_cursors_with_args(
            &cursors,
            &args,
            &mut |v: &mut YSpan, arg| {
                v.status.apply(*arg);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    pub fn retreat(&mut self, spans: &IdSpanVector) {
        if spans.is_empty() {
            return;
        }

        let mut cursors = Vec::with_capacity(spans.len());
        let mut args = Vec::with_capacity(spans.len());
        for span in spans.iter() {
            self.head_vv.set_end(ID::new(*span.0, span.1.start));
            let IdSpanQueryResult { inserts, deletes } = self
                .id_to_cursor
                .get_cursors_at_id_span(IdSpan::new(*span.0, span.1.start, span.1.end));

            for (id, delete) in deletes {
                assert!(span.contains_id(id));
                for deleted_span in delete.iter() {
                    let mut len = 0;
                    for cursor in self
                        .id_to_cursor
                        .get_cursors_at_id_span(*deleted_span)
                        .inserts
                        .into_iter()
                        .map(|x| x.1)
                    {
                        assert!(cursor.len > 0);
                        len += cursor.len;
                        cursors.push(cursor);
                        args.push(StatusChange::UndoDelete);
                    }

                    assert_eq!(len, deleted_span.content_len());
                }
            }

            for span in inserts.into_iter().map(|x| x.1) {
                cursors.push(span);
                args.push(StatusChange::SetAsFuture);
            }
        }

        self.content.update_at_cursors_with_args(
            &cursors,
            &args,
            &mut |v: &mut YSpan, arg| {
                v.status.apply(*arg);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    /// apply an operation directly to the current tracker
    pub(crate) fn apply(&mut self, id: ID, content: &OpContent) {
        assert!(*self.head_vv.get(&id.client_id).unwrap_or(&0) <= id.counter);
        assert!(*self.all_vv.get(&id.client_id).unwrap_or(&0) <= id.counter);
        self.head_vv.set_end(id.inc(content.content_len() as i32));
        self.all_vv.set_end(id.inc(content.content_len() as i32));
        match &content {
            crate::op::OpContent::Normal { content } => {
                let text_content = content.as_list().expect("Content is not for list");
                match text_content {
                    ListOp::Insert { slice, pos } => {
                        let yspan = self.content.get_yspan_at_pos(
                            id,
                            *pos,
                            slice.content_len(),
                            slice.as_slice().unwrap().clone(),
                        );
                        debug_log!("INSERT YSPAN={}", format!("{:#?}", &yspan).red());
                        // SAFETY: we know this is safe because in [YataImpl::insert_after] there is no access to shared elements
                        unsafe { crdt_list::yata::integrate::<YataImpl>(self, yspan) };
                    }
                    ListOp::Delete(span) => {
                        let mut spans = self
                            .content
                            .get_active_id_spans(span.start() as usize, span.atom_len());
                        debug_log!("DELETED SPANS={}", format!("{:#?}", &spans).red());
                        self.update_spans(&spans, StatusChange::Delete);

                        if span.is_reversed() && span.atom_len() > 1 {
                            spans.reverse();
                            // SAFETY: we don't change the size of the span
                            unsafe {
                                for span in spans.iter_mut() {
                                    span.reverse();
                                }
                            }
                        }

                        self.id_to_cursor.set_small_range(
                            (id).into(),
                            cursor_map::Marker::Delete(Box::new(spans)),
                        );
                    }
                }
            }
            crate::op::OpContent::Undo { .. } => todo!(),
            crate::op::OpContent::Redo { .. } => todo!(),
        }
    }

    fn update_cursors(
        &mut self,
        cursor: UnsafeCursor<'_, YSpan, YSpanTreeTrait>,
        change: StatusChange,
    ) -> i32 {
        let mut changed: i32 = 0;
        self.content.update_at_cursors(
            &mut [cursor],
            &mut |v| {
                let before = v.content_len() as i32;
                v.status.apply(change);
                let after = v.content_len() as i32;
                changed += after - before;
            },
            &mut make_notify(&mut self.id_to_cursor),
        );

        changed
    }

    fn update_spans(&mut self, spans: &[IdSpan], change: StatusChange) {
        let mut cursors: SmallVec<[UnsafeCursor<YSpan, YSpanTreeTrait>; 2]> =
            SmallVec::with_capacity(spans.len());
        for span in spans.iter() {
            let inserts = self.id_to_cursor.get_cursors_at_id_span(*span).inserts;
            // TODO: maybe we can skip this collect
            for x in inserts.into_iter() {
                cursors.push(x.1);
            }
        }

        let len = cursors.len();
        self.content.update_at_cursors_with_args(
            &cursors,
            &vec![(); len],
            &mut |v, _| {
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
