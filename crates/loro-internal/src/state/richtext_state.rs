use std::{
    ops::Range,
    sync::{Arc, Mutex, RwLock, Weak},
};

use fxhash::{FxHashMap, FxHashSet};
use generic_btree::rle::HasLength;
use loro_common::{ContainerID, InternalString, LoroResult, LoroValue, ID};
use loro_delta::DeltaRopeBuilder;

use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        list::list_op,
        richtext::{
            config::StyleConfigMap,
            richtext_state::{
                DrainInfo, EntityRangeInfo, IterRangeItem, PosType, RichtextStateChunk,
            },
            AnchorType, RichtextState as InnerState, StyleOp, Styles,
        },
    },
    delta::{StyleMeta, StyleMetaItem},
    encoding::{EncodeMode, StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff, TextDiff},
    handler::TextDelta,
    op::{Op, RawOp},
    txn::Transaction,
    utils::{lazy::LazyLoad, string_slice::StringSlice},
    DocState,
};

use super::ContainerState;

#[derive(Debug)]
pub struct RichtextState {
    idx: ContainerIdx,
    config: Arc<RwLock<StyleConfigMap>>,
    pub(crate) state: LazyLoad<RichtextStateLoader, InnerState>,
}

struct Pos {
    entity_index: usize,
    event_index: usize,
}

impl RichtextState {
    #[inline]
    pub fn new(idx: ContainerIdx, config: Arc<RwLock<StyleConfigMap>>) -> Self {
        Self {
            idx,
            config,
            state: LazyLoad::Src(Default::default()),
        }
    }

    /// Get the text content of the richtext
    ///
    /// This uses `mut` because we may need to build the state from snapshot
    #[inline]
    pub fn to_string_mut(&mut self) -> String {
        self.state.get_mut().to_string()
    }

    #[allow(unused)]
    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        match &self.state {
            LazyLoad::Src(s) => s.elements.is_empty(),
            LazyLoad::Dst(d) => d.is_empty(),
        }
    }

    pub(crate) fn diagnose(&self) {
        match &self.state {
            LazyLoad::Src(_) => {}
            LazyLoad::Dst(d) => d.diagnose(),
        }
    }

    fn get_style_start(
        &mut self,
        style_starts: &mut FxHashMap<Arc<StyleOp>, Pos>,
        style: &Arc<StyleOp>,
    ) -> Pos {
        match style_starts.remove(style) {
            Some(x) => x,
            None => {
                // this should happen rarely, so it should be fine to scan
                let mut pos = Pos {
                    entity_index: 0,
                    event_index: 0,
                };

                for c in self.state.get_mut().iter_chunk() {
                    match c {
                        RichtextStateChunk::Style {
                            style: s,
                            anchor_type: AnchorType::Start,
                        } if style == s => {
                            break;
                        }
                        RichtextStateChunk::Text(t) => {
                            pos.entity_index += t.unicode_len() as usize;
                            pos.event_index += t.event_len() as usize;
                        }
                        RichtextStateChunk::Style { .. } => {
                            pos.entity_index += 1;
                        }
                    }
                }
                pos
            }
        }
    }

    pub fn get_index_of_id(&self, id: ID) -> Option<usize> {
        let iter: &mut dyn Iterator<Item = &RichtextStateChunk>;
        let mut a;
        let mut b;
        match &self.state {
            LazyLoad::Src(s) => {
                a = Some(s.elements.iter());
                iter = &mut *a.as_mut().unwrap();
            }
            LazyLoad::Dst(s) => {
                b = Some(s.iter_chunk());
                iter = &mut *b.as_mut().unwrap();
            }
        }

        let mut index = 0;
        for elem in iter {
            let span = elem.get_id_span();
            if span.contains(id) {
                return Some(index + (id.counter - span.counter.start) as usize);
            }

            index += elem.rle_len();
        }

        None
    }

    pub fn get_event_index_of_id(&self, id: ID) -> Option<usize> {
        let iter: &mut dyn Iterator<Item = &RichtextStateChunk>;
        let mut a;
        let mut b;
        match &self.state {
            LazyLoad::Src(s) => {
                a = Some(s.elements.iter());
                iter = &mut *a.as_mut().unwrap();
            }
            LazyLoad::Dst(s) => {
                b = Some(s.iter_chunk());
                iter = &mut *b.as_mut().unwrap();
            }
        }

        let mut index = 0;
        for elem in iter {
            let span = elem.get_id_span();
            if span.contains(id) {
                match elem {
                    RichtextStateChunk::Text(t) => {
                        let event_offset = t.convert_unicode_offset_to_event_offset(
                            (id.counter - span.counter.start) as usize,
                        );
                        return Some(index + event_offset);
                    }
                    RichtextStateChunk::Style { .. } => {
                        return Some(index);
                    }
                }
            }

            index += match elem {
                RichtextStateChunk::Text(t) => t.event_len() as usize,
                RichtextStateChunk::Style { .. } => 0,
            };
        }

        None
    }

    pub(crate) fn get_delta(&mut self) -> Vec<TextDelta> {
        let mut delta = Vec::new();
        // TODO: merge last
        for span in self.state.get_mut().iter() {
            delta.push(TextDelta::Insert {
                insert: span.text.as_str().to_string(),
                attributes: span.attributes.to_option_map(),
            })
        }
        delta
    }
}

impl Clone for RichtextState {
    fn clone(&self) -> Self {
        Self {
            idx: self.idx,
            config: self.config.clone(),
            state: self.state.clone(),
        }
    }
}

impl ContainerState for RichtextState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        match &self.state {
            LazyLoad::Src(s) => s.elements.len() * std::mem::size_of::<RichtextStateChunk>(),
            LazyLoad::Dst(s) => s.estimate_size(),
        }
    }

    fn is_state_empty(&self) -> bool {
        match &self.state {
            LazyLoad::Src(s) => s.is_empty(),
            LazyLoad::Dst(s) => s.is_empty(),
        }
    }

    // TODO: refactor
    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let InternalDiff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        // tracing::info!("Self state = {:#?}", &self);
        // PERF: compose delta
        let mut ans: TextDiff = TextDiff::new();
        let mut style_delta: TextDiff = TextDiff::new();
        let mut style_starts: FxHashMap<Arc<StyleOp>, Pos> = FxHashMap::default();
        let mut entity_index = 0;
        let mut event_index = 0;
        let mut new_style_deltas: Vec<TextDiff> = Vec::new();
        for span in richtext.vec.iter() {
            match span {
                crate::delta::DeltaItem::Retain { retain: len, .. } => {
                    entity_index += len;
                }
                crate::delta::DeltaItem::Insert { insert: value, .. } => {
                    match value {
                        RichtextStateChunk::Text(s) => {
                            let (pos, styles) = self.state.get_mut().insert_elem_at_entity_index(
                                entity_index,
                                RichtextStateChunk::Text(s.clone()),
                            );
                            let insert_styles = styles.clone().into();

                            if pos > event_index {
                                ans.push_retain(pos - event_index, Default::default());
                            }
                            event_index = pos + s.event_len() as usize;
                            ans.push_insert(StringSlice::from(s.bytes().clone()), insert_styles);
                        }
                        RichtextStateChunk::Style { anchor_type, style } => {
                            let (new_event_index, _) =
                                self.state.get_mut().insert_elem_at_entity_index(
                                    entity_index,
                                    RichtextStateChunk::Style {
                                        style: style.clone(),
                                        anchor_type: *anchor_type,
                                    },
                                );

                            if new_event_index > event_index {
                                ans.push_retain(new_event_index - event_index, Default::default());
                                // inserting style anchor will not affect event_index's positions
                                event_index = new_event_index;
                            }

                            match anchor_type {
                                AnchorType::Start => {
                                    style_starts.insert(
                                        style.clone(),
                                        Pos {
                                            entity_index,
                                            event_index: new_event_index,
                                        },
                                    );
                                }
                                AnchorType::End => {
                                    // get the pair of style anchor. now we can annotate the range
                                    let Pos {
                                        entity_index: start_entity_index,
                                        event_index: start_event_index,
                                    } = self.get_style_start(&mut style_starts, style);
                                    let mut delta: TextDiff = DeltaRopeBuilder::new()
                                        .retain(start_event_index, Default::default())
                                        .build();
                                    // we need to + 1 because we also need to annotate the end anchor
                                    let event =
                                        self.state.get_mut().annotate_style_range_with_event(
                                            start_entity_index..entity_index + 1,
                                            style.clone(),
                                        );
                                    for (s, l) in event {
                                        delta.push_retain(l, s);
                                    }

                                    delta.chop();
                                    new_style_deltas.push(delta);
                                }
                            }
                        }
                    }
                    entity_index += value.rle_len();
                }
                crate::delta::DeltaItem::Delete {
                    delete: len,
                    attributes: _,
                } => {
                    let mut deleted_style_keys: FxHashSet<InternalString> = FxHashSet::default();
                    let DrainInfo {
                        start_event_index: start,
                        end_event_index: end,
                        affected_style_range,
                    } = self.state.get_mut().drain_by_entity_index(
                        entity_index,
                        *len,
                        Some(&mut |c| match c {
                            RichtextStateChunk::Style {
                                style,
                                anchor_type: AnchorType::Start,
                            } => {
                                deleted_style_keys.insert(style.key.clone());
                            }
                            RichtextStateChunk::Style {
                                style,
                                anchor_type: AnchorType::End,
                            } => {
                                deleted_style_keys.insert(style.key.clone());
                            }
                            _ => {}
                        }),
                    );

                    if start > event_index {
                        ans.push_retain(start - event_index, Default::default());
                        event_index = start;
                    }

                    if let Some((entity_range, event_range)) = affected_style_range {
                        let mut delta: TextDiff = DeltaRopeBuilder::new()
                            .retain(event_range.start, Default::default())
                            .build();
                        let mut entity_len_sum = 0;
                        let expected_sum = entity_range.len();

                        for IterRangeItem {
                            event_len,
                            chunk,
                            styles,
                            entity_len,
                            ..
                        } in self.state.get_mut().iter_range(entity_range)
                        {
                            entity_len_sum += entity_len;
                            match chunk {
                                RichtextStateChunk::Text(_) => {
                                    let mut style_meta: StyleMeta = styles.into();
                                    for key in deleted_style_keys.iter() {
                                        if !style_meta.contains_key(key) {
                                            style_meta.insert(
                                                key.clone(),
                                                StyleMetaItem {
                                                    lamport: 0,
                                                    peer: 0,
                                                    value: LoroValue::Null,
                                                },
                                            )
                                        }
                                    }
                                    delta.push_retain(event_len, style_meta);
                                }
                                RichtextStateChunk::Style { .. } => {}
                            }
                        }

                        debug_assert_eq!(entity_len_sum, expected_sum);
                        delta.chop();
                        style_delta.compose(&delta);
                    }

                    ans.push_delete(end - start);
                }
            }
        }

        for s in new_style_deltas {
            style_delta.compose(&s);
        }
        // self.check_consistency_between_content_and_style_ranges();
        ans.compose(&style_delta);
        Diff::Text(ans)
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) {
        let InternalDiff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        let mut style_starts: FxHashMap<Arc<StyleOp>, usize> = FxHashMap::default();
        let mut entity_index = 0;
        for span in richtext.vec.iter() {
            match span {
                crate::delta::DeltaItem::Retain {
                    retain: len,
                    attributes: _,
                } => {
                    entity_index += len;
                }
                crate::delta::DeltaItem::Insert {
                    insert: value,
                    attributes: _,
                } => {
                    match value {
                        RichtextStateChunk::Text(s) => {
                            self.state.get_mut().insert_elem_at_entity_index(
                                entity_index,
                                RichtextStateChunk::Text(s.clone()),
                            );
                        }
                        RichtextStateChunk::Style { style, anchor_type } => {
                            self.state.get_mut().insert_elem_at_entity_index(
                                entity_index,
                                RichtextStateChunk::Style {
                                    style: style.clone(),
                                    anchor_type: *anchor_type,
                                },
                            );

                            if *anchor_type == AnchorType::Start {
                                style_starts.insert(style.clone(), entity_index);
                            } else {
                                let start_pos = match style_starts.get(style) {
                                    Some(x) => *x,
                                    None => {
                                        // This should be rare, so it should be fine to scan
                                        let mut start_entity_index = 0;
                                        for c in self.state.get_mut().iter_chunk() {
                                            match c {
                                                RichtextStateChunk::Style {
                                                    style: s,
                                                    anchor_type: AnchorType::Start,
                                                } if style == s => {
                                                    break;
                                                }
                                                RichtextStateChunk::Text(t) => {
                                                    start_entity_index += t.unicode_len() as usize;
                                                }
                                                RichtextStateChunk::Style { .. } => {
                                                    start_entity_index += 1;
                                                }
                                            }
                                        }
                                        start_entity_index
                                    }
                                };
                                // we need to + 1 because we also need to annotate the end anchor
                                self.state.get_mut().annotate_style_range(
                                    start_pos..entity_index + 1,
                                    style.clone(),
                                );
                            }
                        }
                    }
                    entity_index += value.rle_len();
                }
                crate::delta::DeltaItem::Delete {
                    delete: len,
                    attributes: _,
                } => {
                    self.state
                        .get_mut()
                        .drain_by_entity_index(entity_index, *len, None);
                }
            }
        }

        // self.check_consistency_between_content_and_style_ranges()
    }

    fn apply_local_op(&mut self, r_op: &RawOp, op: &Op) -> LoroResult<()> {
        match &op.content {
            crate::op::InnerContent::List(l) => match l {
                list_op::InnerListOp::Insert { slice: _, pos: _ } => {
                    unreachable!()
                }
                list_op::InnerListOp::InsertText {
                    slice,
                    unicode_len: _,
                    unicode_start: _,
                    pos,
                } => {
                    self.state.get_mut().insert_at_entity_index(
                        *pos as usize,
                        slice.clone(),
                        r_op.id_full(),
                    );
                }
                list_op::InnerListOp::Delete(del) => {
                    self.state.get_mut().drain_by_entity_index(
                        del.start() as usize,
                        rle::HasLength::atom_len(&del),
                        None,
                    );
                }
                list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => {
                    // Behavior here is a little different from apply_diff.
                    //
                    // When apply_diff, we only do the mark when we have included both
                    // StyleStart and StyleEnd.
                    //
                    // When applying local op, we can do the mark when we have StyleStart.
                    // We can assume StyleStart and StyleEnd are always appear in a pair
                    // for apply_local_op. (Because for local behavior, when we mark,
                    // we always create a pair of style ops.)
                    self.state.get_mut().mark_with_entity_index(
                        *start as usize..*end as usize,
                        Arc::new(StyleOp {
                            lamport: r_op.lamport,
                            peer: r_op.id.peer,
                            cnt: r_op.id.counter,
                            key: key.clone(),
                            value: value.clone(),
                            info: *info,
                        }),
                    );
                }
                list_op::InnerListOp::Set { .. } => {}
                list_op::InnerListOp::StyleEnd => {}
                list_op::InnerListOp::Move { .. } => unreachable!(),
            },
            _ => unreachable!(),
        }

        // self.check_consistency_between_content_and_style_ranges();
        Ok(())
    }

    fn to_diff(
        &mut self,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let mut delta = TextDiff::new();
        for span in self.state.get_mut().iter() {
            delta.push_insert(span.text, span.attributes);
        }

        Diff::Text(delta)
    }

    // value is a list
    fn get_value(&mut self) -> LoroValue {
        LoroValue::String(Arc::new(self.state.get_mut().to_string()))
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        None
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        Vec::new()
    }

    fn contains_child(&self, _id: &ContainerID) -> bool {
        false
    }

    #[doc = " Get a list of ops that can be used to restore the state to the current state"]
    fn encode_snapshot(&self, mut encoder: StateSnapshotEncoder) -> Vec<u8> {
        let iter: &mut dyn Iterator<Item = &RichtextStateChunk>;
        let mut a;
        let mut b;
        match &self.state {
            LazyLoad::Src(s) => {
                a = Some(s.elements.iter());
                iter = &mut *a.as_mut().unwrap();
            }
            LazyLoad::Dst(s) => {
                b = Some(s.iter_chunk());
                iter = &mut *b.as_mut().unwrap();
            }
        }

        for chunk in iter {
            let id_span = chunk.get_id_lp_span();
            encoder.encode_op(id_span, || unimplemented!());
        }

        Default::default()
    }

    #[doc = " Restore the state to the state represented by the ops that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        assert_eq!(ctx.mode, EncodeMode::Snapshot);
        let mut loader = RichtextStateLoader::default();
        let mut id_to_style = FxHashMap::default();
        for op in ctx.ops {
            let id = op.id_full();
            let chunk = match op.op.content.into_list().unwrap() {
                list_op::InnerListOp::InsertText { slice, .. } => {
                    RichtextStateChunk::new_text(slice.clone(), id)
                }
                list_op::InnerListOp::StyleStart {
                    key, value, info, ..
                } => {
                    let style_op = Arc::new(StyleOp {
                        lamport: op.lamport.expect("op should already be imported"),
                        peer: op.peer,
                        cnt: op.op.counter,
                        key,
                        value,
                        info,
                    });
                    id_to_style.insert(id, style_op.clone());
                    RichtextStateChunk::new_style(style_op, AnchorType::Start)
                }
                list_op::InnerListOp::StyleEnd => {
                    let style = id_to_style.remove(&id.inc(-1)).unwrap();
                    RichtextStateChunk::new_style(style, AnchorType::End)
                }
                a => unreachable!("richtext state should not have {a:?}"),
            };

            loader.push(chunk);
        }

        self.state = LazyLoad::Src(loader);
        // self.check_consistency_between_content_and_style_ranges();
    }
}

impl RichtextState {
    #[inline(always)]
    pub fn len_utf8(&mut self) -> usize {
        self.state.get_mut().len_utf8()
    }

    #[inline(always)]
    pub fn len_utf16(&mut self) -> usize {
        self.state.get_mut().len_utf16()
    }

    #[inline(always)]
    pub fn len_entity(&self) -> usize {
        match &self.state {
            LazyLoad::Src(s) => s.entity_index,
            LazyLoad::Dst(d) => d.len_entity(),
        }
    }

    pub fn len_event(&mut self) -> usize {
        if cfg!(feature = "wasm") {
            self.len_utf16()
        } else {
            self.len_unicode()
        }
    }

    /// Check if the content and style ranges are consistent.
    ///
    /// Panic if inconsistent.
    #[allow(unused)]
    pub(crate) fn check_consistency_between_content_and_style_ranges(&mut self) {
        if !cfg!(debug_assertions) {
            return;
        }

        self.state
            .get_mut()
            .check_consistency_between_content_and_style_ranges();
    }

    #[inline]
    pub fn len_unicode(&mut self) -> usize {
        self.state.get_mut().len_unicode()
    }

    #[inline]
    pub(crate) fn get_entity_index_for_text_insert(&mut self, event_index: usize) -> usize {
        self.state
            .get_mut()
            .get_entity_index_for_text_insert(event_index, PosType::Event)
    }

    pub(crate) fn get_entity_range_and_styles_at_range(
        &mut self,
        range: Range<usize>,
        pos_type: PosType,
    ) -> (Range<usize>, Option<&Styles>) {
        self.state
            .get_mut()
            .get_entity_range_and_text_styles_at_range(range, pos_type)
    }

    #[inline]
    pub(crate) fn get_styles_at_entity_index(&mut self, entity_index: usize) -> StyleMeta {
        self.state
            .get_mut()
            .get_styles_at_entity_index_for_insert(entity_index)
    }

    #[inline]
    pub(crate) fn get_text_entity_ranges_in_event_index_range(
        &mut self,
        pos: usize,
        len: usize,
    ) -> Vec<EntityRangeInfo> {
        self.state
            .get_mut()
            .get_text_entity_ranges(pos, len, PosType::Event)
    }

    #[inline]
    pub fn get_richtext_value(&mut self) -> LoroValue {
        self.state.get_mut().get_richtext_value()
    }

    #[inline]
    pub(crate) fn get_stable_position(&mut self, event_index: usize) -> Option<ID> {
        self.state
            .get_mut()
            .get_stable_position_at_event_index(event_index, PosType::Event)
    }

    pub(crate) fn entity_index_to_event_index(&mut self, entity_index: usize) -> usize {
        self.state
            .get_mut()
            .entity_index_to_event_index(entity_index)
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct RichtextStateLoader {
    start_anchor_pos: FxHashMap<ID, usize>,
    elements: Vec<RichtextStateChunk>,
    style_ranges: Vec<(Arc<StyleOp>, Range<usize>)>,
    entity_index: usize,
}

impl From<RichtextStateLoader> for InnerState {
    fn from(value: RichtextStateLoader) -> Self {
        value.into_state()
    }
}

impl RichtextStateLoader {
    pub fn push(&mut self, elem: RichtextStateChunk) {
        if let RichtextStateChunk::Style { style, anchor_type } = &elem {
            if *anchor_type == AnchorType::Start {
                self.start_anchor_pos
                    .insert(ID::new(style.peer, style.cnt), self.entity_index);
            } else {
                let start_pos = self
                    .start_anchor_pos
                    .remove(&ID::new(style.peer, style.cnt))
                    .expect("Style start not found");

                // we need to + 1 because we also need to annotate the end anchor
                self.style_ranges
                    .push((style.clone(), start_pos..self.entity_index + 1));
            }
        }

        self.entity_index += elem.rle_len();
        self.elements.push(elem);
    }

    pub fn into_state(self) -> InnerState {
        let mut state = InnerState::from_chunks(self.elements.into_iter());
        for (style, range) in self.style_ranges {
            state.annotate_style_range(range, style);
        }

        if cfg!(debug_assertions) {
            state.check();
        }

        state
    }

    fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}
