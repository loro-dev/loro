use debug_log::debug_dbg;
use rle::{rle_tree::UnsafeCursor, HasLength, Sliceable};
use smallvec::SmallVec;

use crate::{
    container::{list::list_op::InnerListOp, text::tracker::yata_impl::YataImpl},
    delta::Delta,
    id::{Counter, ID},
    op::{InnerContent, RichOp},
    span::{HasId, HasIdSpan, IdSpan},
    version::IdSpanVector,
    VersionVector,
};

#[allow(unused)]
use crate::PeerID;

use self::{
    content_map::ContentMap,
    cursor_map::{make_notify, CursorMap, IdSpanQueryResult},
    effects_iter::EffectIter,
    y_span::{Status, StatusChange, YSpan, YSpanTreeTrait},
};

pub(crate) use effects_iter::Effect;

use super::text_content::{ListSlice, SliceRanges};
mod content_map;
mod cursor_map;
mod effects_iter;
mod y_span;
#[cfg(not(feature = "test_utils"))]
mod yata_impl;
#[cfg(feature = "test_utils")]
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
    #[cfg(feature = "test_utils")]
    client_id: PeerID,
    /// from start_vv to latest vv are applied
    start_vv: VersionVector,
    /// latest applied ops version vector
    all_vv: VersionVector,
    /// current content version vector
    current_vv: VersionVector,
    /// The pretend current content version vector.
    ///
    /// Because sometimes we don't actually need to checkout to the version.
    /// So we may cache the changes then applying them when we really need to.
    content: ContentMap,
    id_to_cursor: CursorMap,
}

// SAFETY: Tracker is safe to be sent to another thread
unsafe impl Send for Tracker {}
// SAFETY: &Tracker is safe to be shared by threads
unsafe impl Sync for Tracker {}

impl From<ID> for u128 {
    fn from(id: ID) -> Self {
        ((id.peer as u128) << 64) | id.counter as u128
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
                    after_status: None,
                    slice: ListSlice::unknown_range(init_len as usize),
                },
                &mut make_notify(&mut id_to_cursor),
            );
        }
        Tracker {
            content,
            id_to_cursor,
            #[cfg(feature = "test_utils")]
            client_id: 0,
            current_vv: start_vv.clone(),
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

    pub fn contains(&self, id: ID) -> bool {
        self.all_vv.includes_id(id)
    }

    /// check whether id_to_cursor correctly reflect the status of the content
    fn check_consistency(&mut self) {
        for span in self.content.iter() {
            let yspan = span.as_ref();
            let id_span = IdSpan::new(
                yspan.id.peer,
                yspan.id.counter,
                yspan.atom_len() as Counter + yspan.id.counter,
            );
            let mut len = 0;
            for marker in self
                .id_to_cursor
                .get_range(id_span.norm_id_start().into(), id_span.norm_id_end().into())
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

    pub fn checkout(&mut self, vv: &VersionVector) {
        self._checkout(vv, false)
    }

    /// for_diff = true should be called after the tracker checkout to A version with for_diff = false.
    /// Then we can calculate the diff between A and vv.  
    fn _checkout(&mut self, vv: &VersionVector, for_diff: bool) {
        // clear after_status as it may be outdated
        if for_diff {
            for mut span in self.content.iter_mut() {
                span.as_mut().after_status = None;
            }
        }

        if &self.current_vv == vv {
            // we can return here even if in for_diff mode.
            // because by default after_status will use the status in the current version
            return;
        }

        debug_dbg!(&self.current_vv, &vv);
        let self_vv = std::mem::take(&mut self.current_vv);
        {
            let diff = self_vv.diff_iter(vv);
            self.retreat(diff.0, for_diff);
            self.forward(diff.1, for_diff);
        }

        if for_diff {
            // if it's for_diff, current_version is not changed, so it should be reset to its old value
            self.current_vv = self_vv;
        } else {
            self.current_vv = vv.clone();
        }

        debug_dbg!(&self.current_vv, &vv);
    }

    pub fn diff(&mut self, from: &VersionVector, to: &VersionVector) -> Delta<SliceRanges, ()> {
        self._checkout(from, false);
        self._checkout(to, true);
        let mut ans = Delta::new();
        for span in self.content.iter() {
            let s = span.as_ref();
            debug_dbg!(&s);
            match s.status_diff() {
                y_span::StatusDiff::New => {
                    let v: SliceRanges = s.slice.clone().into();
                    ans = ans.insert(v);
                }
                y_span::StatusDiff::Delete => ans = ans.delete(s.slice.atom_len()),
                y_span::StatusDiff::Unchanged => {
                    ans = ans.retain(s.content_len());
                }
            }
        }

        ans.chop()
    }

    pub fn track_apply(&mut self, rich_op: &RichOp) {
        let content = rich_op.get_sliced().content;
        let id = rich_op.id_start();
        if self
            .all_vv()
            .includes_id(id.inc(content.atom_len() as Counter - 1))
        {
            self.forward(std::iter::once(id.to_span(content.atom_len())), false);
            return;
        }

        if self.all_vv().includes_id(id) {
            let this_ctr = self.all_vv().get(&id.peer).unwrap();
            let shift = this_ctr - id.counter;
            self.forward(std::iter::once(id.to_span(shift as usize)), false);
            if shift as usize >= content.atom_len() {
                unreachable!();
            }
            self.apply(
                id.inc(shift),
                &content.slice(shift as usize, content.atom_len()),
            );
        } else {
            self.apply(id, &content)
        }
    }

    fn forward(&mut self, spans: impl Iterator<Item = IdSpan>, for_diff: bool) {
        let mut cursors = Vec::new();
        let mut args = Vec::new();
        for span in spans {
            debug_log::group!("forward {:?}", &span);
            let end_id = ID::new(span.client_id, span.counter.end);
            if !for_diff {
                self.current_vv.set_end(end_id);
            }
            if let Some(all_end_ctr) = self.all_vv.get(&span.client_id) {
                let all_end = *all_end_ctr;
                if all_end < span.counter.end {
                    // there may be holes when there are multiple containers
                    self.all_vv.insert(span.client_id, span.counter.end);
                }
                if all_end <= span.counter.start {
                    continue;
                }
            } else {
                self.all_vv.set_end(end_id);
                continue;
            }

            let IdSpanQueryResult { inserts, deletes } = self.id_to_cursor.get_cursors_at_id_span(
                IdSpan::new(span.client_id, span.counter.start, span.counter.end),
            );
            debug_dbg!(&deletes);
            for (_, delete) in deletes {
                for deleted_span in delete.iter() {
                    for span in self
                        .id_to_cursor
                        .get_cursors_at_id_span(*deleted_span)
                        .inserts
                        .into_iter()
                        .map(|x| x.1)
                    {
                        debug_dbg!(&span);
                        cursors.push(span);
                        args.push(StatusChange::Delete);
                    }
                }
            }

            for span in inserts.into_iter().map(|x| x.1) {
                cursors.push(span);
                args.push(StatusChange::SetAsCurrent);
            }
            debug_log::group_end!();
        }

        self.content.update_at_cursors_with_args(
            &cursors,
            &args,
            &mut |v: &mut YSpan, arg| {
                debug_dbg!(&v);
                if !for_diff {
                    v.status.apply(*arg);
                } else {
                    if v.after_status.is_none() {
                        v.after_status = Some(v.status);
                    }

                    v.after_status.as_mut().unwrap().apply(*arg);
                }
                debug_dbg!(&v);
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    fn retreat(&mut self, spans: impl Iterator<Item = IdSpan>, for_diff: bool) {
        let mut cursors = Vec::new();
        let mut args = Vec::new();
        for span in spans {
            debug_dbg!("retreat", &span);
            let span_start = ID::new(span.client_id, span.counter.start);
            if !for_diff {
                self.current_vv.set_end(span_start);
            }
            if let Some(all_end_ctr) = self.all_vv.get(&span.client_id) {
                let all_end = *all_end_ctr;
                if all_end < span.counter.start {
                    self.all_vv.insert(span.client_id, span.counter.end);
                    continue;
                }
            } else {
                self.all_vv.set_end(span_start);
                continue;
            }

            let IdSpanQueryResult { inserts, deletes } = self.id_to_cursor.get_cursors_at_id_span(
                IdSpan::new(span.client_id, span.counter.start, span.counter.end),
            );

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
                if !for_diff {
                    v.status.apply(*arg);
                } else {
                    if v.after_status.is_none() {
                        v.after_status = Some(v.status);
                    }

                    v.after_status.as_mut().unwrap().apply(*arg);
                }
            },
            &mut make_notify(&mut self.id_to_cursor),
        )
    }

    /// apply an operation directly to the current tracker
    fn apply(&mut self, id: ID, content: &InnerContent) {
        assert!(*self.current_vv.get(&id.peer).unwrap_or(&0) <= id.counter);
        assert!(*self.all_vv.get(&id.peer).unwrap_or(&0) <= id.counter);
        self.current_vv
            .set_end(id.inc(content.content_len() as i32));
        self.all_vv.set_end(id.inc(content.content_len() as i32));
        let text_content = content.as_list().expect("Content is not for list");
        match text_content {
            InnerListOp::Insert { slice, pos } => {
                let yspan =
                    self.content
                        .get_yspan_at_pos(id, *pos, slice.content_len(), slice.clone());
                self.with_context(|this, context| {
                    crdt_list::yata::integrate::<YataImpl>(this, yspan, context)
                });
            }
            InnerListOp::Delete(span) => {
                let mut spans = self
                    .content
                    .get_active_id_spans(span.start() as usize, span.atom_len());
                debug_log::debug_log!("DELETED SPANS={}", format!("{:?}", &spans));
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

                self.id_to_cursor
                    .set_small_range((id).into(), cursor_map::Marker::Delete(Box::new(spans)));
            }
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

    pub fn iter_effects(&mut self, from: &VersionVector, target: &IdSpanVector) -> EffectIter<'_> {
        self.checkout(from);
        EffectIter::new(self, target)
    }

    pub fn check(&mut self) {
        self.check_consistency();
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }
}
