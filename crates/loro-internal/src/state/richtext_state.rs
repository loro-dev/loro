use std::{ops::Range, sync::Arc};

use fxhash::FxHashMap;
use generic_btree::rle::{HasLength, Mergeable};
use loro_common::{Counter, LoroResult, LoroValue, PeerID, ID};
use loro_preload::{CommonArena, EncodedRichtextState, TempArena, TextRanges};

use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        richtext::{
            richtext_state::{EntityRangeInfo, PosType},
            AnchorType, RichtextState as InnerState, StyleOp, Styles, TextStyleInfoFlag,
        },
    },
    container::{list::list_op, richtext::richtext_state::RichtextStateChunk},
    delta::{Delta, DeltaItem, StyleMeta},
    event::{Diff, InternalDiff},
    op::{Op, RawOp},
    utils::{bitmap::BitMap, lazy::LazyLoad, string_slice::StringSlice},
    InternalString,
};

use super::ContainerState;

#[derive(Debug)]
pub struct RichtextState {
    idx: ContainerIdx,
    pub(crate) state: Box<LazyLoad<RichtextStateLoader, InnerState>>,
    in_txn: bool,
    undo_stack: Vec<UndoItem>,
}

impl RichtextState {
    #[inline]
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            state: Box::new(LazyLoad::new_dst(Default::default())),
            in_txn: false,
            undo_stack: Default::default(),
        }
    }

    /// Get the text content of the richtext
    ///
    /// This uses `mut` because we may need to build the state from snapshot
    #[inline]
    pub fn to_string_mut(&mut self) -> String {
        self.state.get_mut().to_string()
    }

    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        match &*self.state {
            LazyLoad::Src(s) => s.elements.is_empty(),
            LazyLoad::Dst(d) => d.is_emtpy(),
        }
    }

    pub(crate) fn diagnose(&self) {
        match &*self.state {
            LazyLoad::Src(_) => {}
            LazyLoad::Dst(d) => d.diagnose(),
        }
    }
}

impl Clone for RichtextState {
    fn clone(&self) -> Self {
        Self {
            idx: self.idx,
            state: self.state.clone(),
            in_txn: false,
            undo_stack: Vec::new(),
        }
    }
}

#[derive(Debug)]
enum UndoItem {
    Insert {
        index: u32,
        len: u32,
    },
    Delete {
        index: u32,
        content: RichtextStateChunk,
    },
}

impl Mergeable for UndoItem {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (UndoItem::Insert { index, len }, UndoItem::Insert { index: r_index, .. }) => {
                *index + *len == *r_index
            }
            (
                UndoItem::Delete { index, content },
                UndoItem::Delete {
                    index: r_i,
                    content: r_c,
                },
            ) => *r_i + r_c.rle_len() as u32 == *index && r_c.can_merge(content),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (UndoItem::Insert { len, .. }, UndoItem::Insert { len: r_len, .. }) => {
                *len += *r_len;
            }
            (
                UndoItem::Delete { content, index },
                UndoItem::Delete {
                    content: r_c,
                    index: r_i,
                },
            ) => {
                if *r_i + r_c.rle_len() as u32 == *index {
                    content.merge_right(r_c);
                    *index = *r_i
                }
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, _: &Self) {
        unreachable!()
    }
}

impl ContainerState for RichtextState {
    // TODO: refactor
    fn apply_diff_and_convert(&mut self, diff: InternalDiff, _arena: &SharedArena) -> Diff {
        let InternalDiff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        debug_log::group!("apply_diff_and_convert");
        debug_log::debug_dbg!(&richtext);
        // PERF: compose delta
        let mut ans: Delta<StringSlice, StyleMeta> = Delta::new();
        let mut style_delta: Delta<StringSlice, StyleMeta> = Delta::new();
        struct Pos {
            entity_index: usize,
            event_index: usize,
        }

        let mut style_starts: FxHashMap<Arc<StyleOp>, Pos> = FxHashMap::default();
        let mut entity_index = 0;
        let mut event_index = 0;
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
                                ans = ans.retain(pos - event_index);
                            }
                            event_index = pos + s.event_len() as usize;
                            ans = ans.insert_with_meta(
                                StringSlice::from(s.bytes().clone()),
                                insert_styles,
                            );
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
                                ans = ans.retain(new_event_index - event_index);
                                // inserting style anchor will not affect event_index's positions
                                event_index = new_event_index;
                            }

                            if *anchor_type == AnchorType::Start {
                                style_starts.insert(
                                    style.clone(),
                                    Pos {
                                        entity_index,
                                        event_index: new_event_index,
                                    },
                                );
                            } else {
                                // get the pair of style anchor. now we can annotate the range
                                let Pos {
                                    entity_index: start_entity_index,
                                    event_index: start_event_index,
                                } = style_starts.remove(style).unwrap();

                                // we need to + 1 because we also need to annotate the end anchor
                                self.state.get_mut().annotate_style_range(
                                    start_entity_index..entity_index + 1,
                                    style.clone(),
                                );

                                let mut meta = StyleMeta::default();

                                meta.insert(
                                    style.get_style_key(),
                                    crate::delta::StyleMetaItem {
                                        lamport: style.lamport,
                                        peer: style.peer,
                                        value: style.to_value(),
                                    },
                                );
                                let delta: Delta<StringSlice, _> = Delta::new()
                                    .retain(start_event_index)
                                    .retain_with_meta(new_event_index - start_event_index, meta);
                                debug_log::debug_dbg!(&delta);
                                style_delta = style_delta.compose(delta);
                            }
                        }
                    }
                    entity_index += value.rle_len();
                }
                crate::delta::DeltaItem::Delete {
                    delete: len,
                    attributes: _,
                } => {
                    let (start, end) =
                        self.state
                            .get_mut()
                            .drain_by_entity_index(entity_index, *len, |_| {});
                    if start > event_index {
                        ans = ans.retain(start - event_index);
                        event_index = start;
                    }

                    ans = ans.delete(end - start);
                }
            }
        }

        debug_assert!(style_starts.is_empty(), "Styles should always be paired");
        debug_log::debug_dbg!(&ans, &style_delta);
        let ans = ans.compose(style_delta);
        debug_log::debug_dbg!(&ans);
        debug_log::group_end!();
        Diff::Text(ans)
    }

    fn apply_diff(&mut self, diff: InternalDiff, _arena: &SharedArena) {
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
                                let start_pos =
                                    style_starts.get(style).expect("Style start not found");
                                // we need to + 1 because we also need to annotate the end anchor
                                self.state.get_mut().annotate_style_range(
                                    *start_pos..entity_index + 1,
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
                        .drain_by_entity_index(entity_index, *len, |_| {});
                }
            }
        }
    }

    fn apply_op(&mut self, r_op: &RawOp, op: &Op, _arena: &SharedArena) -> LoroResult<()> {
        match &op.content {
            crate::op::InnerContent::List(l) => match l {
                list_op::InnerListOp::Insert { slice: _, pos: _ } => {
                    unreachable!()
                }
                list_op::InnerListOp::InsertText {
                    slice,
                    unicode_len: len,
                    unicode_start: _,
                    pos,
                } => {
                    self.state
                        .get_mut()
                        .insert_at_entity_index(*pos as usize, slice.clone());

                    if self.in_txn {
                        self.push_undo(UndoItem::Insert {
                            index: *pos,
                            len: *len,
                        })
                    }
                }
                list_op::InnerListOp::Delete(del) => {
                    self.state.get_mut().drain_by_entity_index(
                        del.start() as usize,
                        rle::HasLength::atom_len(&del),
                        |span| {
                            if self.in_txn {
                                let item = UndoItem::Delete {
                                    index: del.start() as u32,
                                    content: span,
                                };
                                match self.undo_stack.last_mut() {
                                    Some(last) if last.can_merge(&item) => {
                                        last.merge_right(&item);
                                    }
                                    _ => {
                                        self.undo_stack.push(item);
                                    }
                                }
                            }
                        },
                    );
                }
                list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => {
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
                list_op::InnerListOp::StyleEnd => {}
            },
            _ => unreachable!(),
        }
        Ok(())
    }

    fn to_diff(&mut self) -> Diff {
        let mut delta = crate::delta::Delta::new();
        for span in self.state.get_mut().iter() {
            delta.vec.push(DeltaItem::Insert {
                insert: span.text,
                attributes: span.attributes,
            })
        }

        Diff::Text(delta)
    }

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        self.undo_all();
    }

    fn commit_txn(&mut self) {
        self.in_txn = false;
        self.undo_stack.clear();
    }

    // value is a list
    fn get_value(&mut self) -> LoroValue {
        LoroValue::String(Arc::new(self.state.get_mut().to_string()))
    }
}

impl RichtextState {
    fn undo_all(&mut self) {
        while let Some(item) = self.undo_stack.pop() {
            match item {
                UndoItem::Insert { index, len } => {
                    self.state.get_mut().drain_by_entity_index(
                        index as usize,
                        len as usize,
                        |_| {},
                    );
                }
                UndoItem::Delete { index, content } => {
                    match content {
                        RichtextStateChunk::Text { .. } => {}
                        RichtextStateChunk::Style { .. } => {
                            unimplemented!("should handle style annotation")
                        }
                    }

                    self.state
                        .get_mut()
                        .insert_elem_at_entity_index(index as usize, content);
                }
            }
        }
    }

    fn push_undo(&mut self, item: UndoItem) {
        match self.undo_stack.last_mut() {
            Some(last) if last.can_merge(&item) => {
                last.merge_right(&item);
            }
            _ => {
                self.undo_stack.push(item);
            }
        }
    }

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
        match &*self.state {
            LazyLoad::Src(s) => s.entity_index,
            LazyLoad::Dst(d) => d.len_entity(),
        }
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
    fn get_loader() -> RichtextStateLoader {
        RichtextStateLoader {
            elements: Default::default(),
            start_anchor_pos: Default::default(),
            entity_index: 0,
            style_ranges: Default::default(),
        }
    }

    #[inline]
    pub(crate) fn iter_chunk(&self) -> Box<dyn Iterator<Item = &RichtextStateChunk> + '_> {
        match &*self.state {
            LazyLoad::Src(s) => Box::new(s.elements.iter()),
            LazyLoad::Dst(s) => Box::new(s.iter_chunk()),
        }
    }

    pub(crate) fn decode_snapshot(
        &mut self,
        EncodedRichtextState {
            len,
            text_bytes,
            styles,
            is_style_start,
        }: EncodedRichtextState,
        state_arena: &TempArena,
        common: &CommonArena,
        arena: &SharedArena,
    ) {
        assert!(self.is_empty());
        if text_bytes.is_empty() {
            return;
        }

        let bit_len = is_style_start.len() * 8;
        let is_style_start = BitMap::from_vec(is_style_start, bit_len);
        let mut is_style_start_iter = is_style_start.iter();
        let mut loader = Self::get_loader();
        let mut is_text = true;
        let mut text_range_iter = TextRanges::decode_iter(&text_bytes).unwrap();
        let mut style_iter = styles.iter();
        for &len in len.iter() {
            if is_text {
                for _ in 0..len {
                    let range = text_range_iter.next().unwrap();
                    let text = arena.slice_by_utf8(range.start..range.start + range.len);
                    loader.push(RichtextStateChunk::new_text(text));
                }
            } else {
                for _ in 0..len {
                    let is_start = is_style_start_iter.next().unwrap();
                    let style_compact = style_iter.next().unwrap();
                    loader.push(RichtextStateChunk::new_style(
                        Arc::new(StyleOp {
                            lamport: style_compact.lamport,
                            peer: common.peer_ids[style_compact.peer_idx as usize],
                            cnt: style_compact.counter as Counter,
                            key: state_arena.keywords[style_compact.key_idx as usize].clone(),
                            value: style_compact.value.clone(),
                            info: TextStyleInfoFlag::from_byte(style_compact.style_info),
                        }),
                        if is_start {
                            AnchorType::Start
                        } else {
                            AnchorType::End
                        },
                    ))
                }
            }

            is_text = !is_text;
        }

        self.state = Box::new(LazyLoad::new(loader));
    }

    pub(crate) fn encode_snapshot(
        &self,
        record_peer: &mut impl FnMut(PeerID) -> u32,
        record_key: &mut impl FnMut(&InternalString) -> usize,
    ) -> EncodedRichtextState {
        // lengths are interleaved [text_elem_len, style_elem_len, ..]
        let mut lengths = Vec::new();
        let mut text_ranges: TextRanges = Default::default();
        let mut styles = Vec::new();
        let mut is_style_start = BitMap::new();

        for chunk in self.iter_chunk() {
            match chunk {
                RichtextStateChunk::Text(s) => {
                    if lengths.len() % 2 == 0 {
                        lengths.push(0);
                    }

                    *lengths.last_mut().unwrap() += 1;
                    text_ranges.ranges.push(loro_preload::TextRange {
                        start: s.bytes().start(),
                        len: s.bytes().len(),
                    });
                }
                RichtextStateChunk::Style { style, anchor_type } => {
                    if lengths.is_empty() {
                        lengths.reserve(2);
                        lengths.push(0);
                        lengths.push(0);
                    }

                    if lengths.len() % 2 == 1 {
                        lengths.push(0);
                    }

                    *lengths.last_mut().unwrap() += 1;
                    is_style_start.push(*anchor_type == AnchorType::Start);
                    styles.push(loro_preload::CompactStyleOp {
                        peer_idx: record_peer(style.peer),
                        key_idx: record_key(&style.key) as u32,
                        counter: style.cnt as u32,
                        lamport: style.lamport,
                        style_info: style.info.to_byte(),
                        value: style.value.clone(),
                    })
                }
            }
        }

        let text_bytes = text_ranges.encode();
        // eprintln!("bytes len={}", text_bytes.len());
        EncodedRichtextState {
            len: lengths,
            text_bytes: std::borrow::Cow::Owned(text_bytes),
            styles,
            is_style_start: is_style_start.into_vec(),
        }
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
                debug_log::debug_dbg!(&self.start_anchor_pos);
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
        debug_log::debug_dbg!(&self);
        let mut state = InnerState::from_chunks(self.elements.into_iter());
        for (style, range) in self.style_ranges {
            state.annotate_style_range(range, style);
        }

        if cfg!(debug_assertions) {
            state.check();
        }

        state
    }
}
