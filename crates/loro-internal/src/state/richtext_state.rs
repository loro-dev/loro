use std::{ops::Range, sync::Arc};

use fxhash::FxHashMap;
use generic_btree::rle::HasLength;
use loro_common::{Counter, LoroValue, PeerID, ID};
use loro_preload::{CommonArena, EncodedRichtextState, TempArena, TextRanges};

use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        richtext::{
            query::{EventIndexQuery, EventIndexQueryT},
            AnchorType, RichtextState as InnerState, StyleOp, TextStyleInfoFlag,
        },
    },
    container::{list::list_op, richtext::richtext_state::RichtextStateChunk},
    delta::{Delta, DeltaItem, StyleMeta},
    event::{Diff, InternalDiff},
    op::{Op, RawOp},
    utils::{bitmap::BitMap, lazy::LazyLoad, string_slice::StringSlice, utf16::count_utf16_chars},
    InternalString,
};

use super::ContainerState;

#[derive(Debug)]
pub struct RichtextState {
    idx: ContainerIdx,
    state: LazyLoad<RichtextStateLoader, InnerState>,
    in_txn: bool,
    undo_stack: Vec<UndoItem>,
}

impl RichtextState {
    #[inline]
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            state: LazyLoad::new_dst(Default::default()),
            in_txn: false,
            undo_stack: Default::default(),
        }
    }

    #[inline]
    pub fn to_string(&mut self) -> String {
        self.state.get_mut().to_string()
    }

    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        match &self.state {
            LazyLoad::Src(s) => s.elements.is_empty(),
            LazyLoad::Dst(d) => d.is_emtpy(),
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

impl ContainerState for RichtextState {
    // TODO: refactor
    fn apply_diff_and_convert(&mut self, diff: InternalDiff, _arena: &SharedArena) -> Diff {
        let InternalDiff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        debug_log::group!("apply richtext diff and convert");
        debug_log::debug_dbg!(&richtext);
        let mut ans: Delta<StringSlice, StyleMeta> = Delta::new();
        let mut entity_index = 0;
        let mut event_index = 0;
        let mut last_style_index = 0;
        let mut style_starts: FxHashMap<Arc<StyleOp>, usize> = FxHashMap::default();
        for span in richtext.vec.iter() {
            match span {
                crate::delta::DeltaItem::Retain { len, .. } => {
                    entity_index += len;
                }
                crate::delta::DeltaItem::Insert { value, .. } => {
                    match value {
                        RichtextStateChunk::Text { unicode_len, text } => {
                            let (pos, styles) = self.state.get_mut().insert_elem_at_entity_index(
                                entity_index,
                                RichtextStateChunk::Text {
                                    unicode_len: *unicode_len,
                                    text: text.clone(),
                                },
                            );
                            let insert_styles = StyleMeta {
                                vec: styles
                                    .iter()
                                    .flat_map(|(_, value)| value.to_styles())
                                    .collect(),
                            };

                            if pos > event_index {
                                let mut new_len = 0;
                                for (len, styles) in self
                                    .state
                                    .get_mut()
                                    .iter_styles_in_event_index_range(event_index..pos)
                                {
                                    new_len += len;
                                    ans = ans.retain_with_meta(
                                        len,
                                        StyleMeta {
                                            vec: styles
                                                .iter()
                                                .flat_map(|(_, value)| value.to_styles())
                                                .collect(),
                                        },
                                    );
                                }

                                assert_eq!(new_len, pos - event_index);
                            }
                            event_index = pos
                                + (if cfg!(feature = "wasm") {
                                    count_utf16_chars(text)
                                } else {
                                    *unicode_len as usize
                                });
                            ans = ans
                                .insert_with_meta(StringSlice::from(text.clone()), insert_styles);
                        }
                        RichtextStateChunk::Style { style, anchor_type } => {
                            match anchor_type {
                                AnchorType::Start => {}
                                AnchorType::End => {
                                    last_style_index = event_index;
                                }
                            }
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
                    self.undo_stack.push(UndoItem::Insert {
                        index: entity_index as u32,
                        len: value.rle_len() as u32,
                    });
                    entity_index += value.rle_len();
                }
                crate::delta::DeltaItem::Delete { len, meta: _ } => {
                    let (content, start, end) = self
                        .state
                        .get_mut()
                        .drain_by_entity_index(entity_index, *len);
                    for span in content {
                        self.undo_stack.push(UndoItem::Delete {
                            index: entity_index as u32,
                            content: span,
                        })
                    }
                    if start > event_index {
                        for (len, styles) in self
                            .state
                            .get_mut()
                            .iter_styles_in_event_index_range(event_index..start)
                        {
                            ans = ans.retain_with_meta(
                                len,
                                StyleMeta {
                                    vec: styles
                                        .iter()
                                        .flat_map(|(_, value)| value.to_styles())
                                        .collect(),
                                },
                            );
                        }

                        event_index = start;
                    }

                    ans = ans.delete(end - start);
                }
            }
        }

        if last_style_index > event_index {
            for (len, styles) in self
                .state
                .get_mut()
                .iter_styles_in_event_index_range(event_index..last_style_index)
            {
                ans = ans.retain_with_meta(
                    len,
                    StyleMeta {
                        vec: styles
                            .iter()
                            .flat_map(|(_, value)| value.to_styles())
                            .collect(),
                    },
                );
            }
        }

        debug_log::debug_dbg!(&ans);
        debug_log::group_end!();
        Diff::Text(ans)
    }

    fn apply_diff(&mut self, diff: InternalDiff, _arena: &SharedArena) {
        let InternalDiff::RichtextRaw(richtext) = diff else {
            unreachable!()
        };

        debug_log::debug_dbg!(&richtext);
        let mut style_starts: FxHashMap<Arc<StyleOp>, usize> = FxHashMap::default();
        let mut entity_index = 0;
        for span in richtext.vec.iter() {
            match span {
                crate::delta::DeltaItem::Retain { len, meta: _ } => {
                    entity_index += len;
                }
                crate::delta::DeltaItem::Insert { value, meta: _ } => {
                    match value {
                        RichtextStateChunk::Text { unicode_len, text } => {
                            self.state.get_mut().insert_elem_at_entity_index(
                                entity_index,
                                RichtextStateChunk::Text {
                                    unicode_len: *unicode_len,
                                    text: text.clone(),
                                },
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
                    self.undo_stack.push(UndoItem::Insert {
                        index: entity_index as u32,
                        len: value.rle_len() as u32,
                    });
                    entity_index += value.rle_len();
                }
                crate::delta::DeltaItem::Delete { len, meta: _ } => {
                    let (content, _start, _end) = self
                        .state
                        .get_mut()
                        .drain_by_entity_index(entity_index, *len);
                    for span in content {
                        self.undo_stack.push(UndoItem::Delete {
                            index: entity_index as u32,
                            content: span,
                        })
                    }
                }
            }
        }
    }

    fn apply_op(&mut self, r_op: &RawOp, op: &Op, arena: &SharedArena) {
        match &op.content {
            crate::op::InnerContent::List(l) => match l {
                list_op::InnerListOp::Insert { slice, pos } => {
                    self.state.get_mut().insert_at_entity_index(
                        *pos,
                        arena.slice_by_unicode(slice.0.start as usize..slice.0.end as usize),
                    );

                    if self.in_txn {
                        self.undo_stack.push(UndoItem::Insert {
                            index: *pos as u32,
                            len: slice.0.end - slice.0.start,
                        })
                    }
                }
                list_op::InnerListOp::Delete(del) => {
                    for span in self
                        .state
                        .get_mut()
                        .drain_by_entity_index(del.start() as usize, rle::HasLength::atom_len(&del))
                        .0
                    {
                        if self.in_txn {
                            self.undo_stack.push(UndoItem::Delete {
                                index: del.start() as u32,
                                content: span,
                            })
                        }
                    }
                }
                list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    info,
                } => {
                    self.state.get_mut().mark_with_entity_index(
                        *start as usize..*end as usize,
                        Arc::new(StyleOp {
                            lamport: r_op.lamport,
                            peer: r_op.id.peer,
                            cnt: r_op.id.counter,
                            key: key.clone(),
                            info: *info,
                        }),
                    );
                }
                list_op::InnerListOp::StyleEnd => {}
            },
            _ => unreachable!(),
        }
    }

    fn to_diff(&mut self) -> Diff {
        let mut delta = crate::delta::Delta::new();
        for span in self.state.get_mut().iter() {
            delta.vec.push(DeltaItem::Insert {
                value: span.text,
                meta: StyleMeta { vec: span.styles },
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
                    let _ = self
                        .state
                        .get_mut()
                        .drain_by_entity_index(index as usize, len as usize);
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

    #[inline(always)]
    pub fn len_unicode(&mut self) -> usize {
        self.state.get_mut().len_unicode()
    }

    #[inline(always)]
    pub(crate) fn get_entity_index_for_text_insert_event_index(&mut self, pos: usize) -> usize {
        self.state
            .get_mut()
            .get_entity_index_for_text_insert::<EventIndexQueryT>(pos)
    }

    #[inline(always)]
    pub(crate) fn get_text_entity_ranges_in_event_index_range(
        &mut self,
        pos: usize,
        len: usize,
    ) -> Vec<Range<usize>> {
        self.state
            .get_mut()
            .get_text_entity_ranges::<EventIndexQuery>(pos, len)
    }

    #[inline(always)]
    pub fn get_richtext_value(&mut self) -> LoroValue {
        self.state.get_mut().get_richtext_value()
    }

    #[inline(always)]
    fn get_loader() -> RichtextStateLoader {
        RichtextStateLoader {
            elements: Default::default(),
            start_anchor_pos: Default::default(),
            entity_index: 0,
            style_ranges: Default::default(),
        }
    }

    #[inline(always)]
    pub(crate) fn iter_chunk(&self) -> Box<dyn Iterator<Item = &RichtextStateChunk> + '_> {
        match &self.state {
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

        self.state = LazyLoad::new(loader);
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
                RichtextStateChunk::Text {
                    text,
                    unicode_len: _,
                } => {
                    if lengths.len() % 2 == 0 {
                        lengths.push(0);
                    }

                    *lengths.last_mut().unwrap() += 1;
                    text_ranges.ranges.push(loro_preload::TextRange {
                        start: text.start(),
                        len: text.len(),
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
        debug_log::debug_dbg!(&elem);
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

        debug_log::debug_dbg!(&state);
        if cfg!(debug_assertions) {
            state.check();
        }

        state
    }
}
