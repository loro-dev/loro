use core::panic;
use std::{
    borrow::Cow,
    mem::take,
    sync::{Arc, Weak},
};

use enum_as_inner::EnumAsInner;
use generic_btree::rle::{HasLength as RleHasLength, Mergeable as GBSliceable};
use loro_common::{ContainerType, IdLp, IdSpan, LoroResult};
use loro_delta::{array_vec::ArrayVec, DeltaRopeBuilder};
use rle::{HasLength, Mergable, RleVec};
use smallvec::{smallvec, SmallVec};

use crate::{
    change::{Change, Lamport, Timestamp},
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, InnerListOp},
        richtext::Style,
        IntoContainerId,
    },
    delta::{ResolvedMapDelta, ResolvedMapValue, StyleMeta, StyleMetaItem, TreeDiff, TreeDiffItem},
    encoding::export_fast_updates_in_range,
    event::{Diff, ListDeltaMeta, TextDiff},
    handler::{Handler, ValueOrHandler},
    id::{Counter, PeerID, ID},
    lock::LoroMutex,
    loro::CommitOptions,
    op::{Op, RawOp, RawOpContent},
    pre_commit::{ChangeModifier, PreCommitCallbackPayload},
    span::HasIdSpan,
    version::Frontiers,
    ChangeMeta, InternalString, LoroDoc, LoroDocInner, LoroError, LoroValue,
};

use super::{
    arena::SharedArena,
    event::{InternalContainerDiff, InternalDocDiff},
    handler::{ListHandler, MapHandler, TextHandler, TreeHandler},
    oplog::OpLog,
    state::DocState,
};

impl crate::LoroDoc {
    /// Create a new transaction.
    /// Every ops created inside one transaction will be packed into a single
    /// [Change].
    ///
    /// There can only be one active transaction at a time for a [LoroDoc].
    #[inline(always)]
    pub fn txn(&self) -> Result<Transaction, LoroError> {
        self.txn_with_origin("")
    }

    /// Create a new transaction with specified origin.
    ///
    /// The origin will be propagated to the events.
    /// There can only be one active transaction at a time for a [LoroDoc].
    pub fn txn_with_origin(&self, origin: &str) -> Result<Transaction, LoroError> {
        if !self.can_edit() {
            return Err(LoroError::TransactionError(
                String::from("LoroDoc is in readonly detached mode. To make it writable in detached mode, call `set_detached_editing(true)`.").into_boxed_str(),
            ));
        }

        let mut txn = Transaction::new_with_origin(self.inner.clone(), origin.into());

        let obs = self.observer.clone();
        let local_update_subs_weak = self.local_update_subs.downgrade();
        txn.set_on_commit(Box::new(move |state, oplog, id_span| {
            let mut state = state.lock().unwrap();
            let events = state.take_events();
            drop(state);
            for event in events {
                obs.emit(event);
            }

            if id_span.atom_len() == 0 {
                return;
            }

            if let Some(local_update_subs) = local_update_subs_weak.upgrade() {
                if !local_update_subs.inner().is_empty() {
                    let bytes =
                        { export_fast_updates_in_range(&oplog.lock().unwrap(), &[id_span]) };
                    local_update_subs.emit(&(), bytes);
                }
            }
        }));

        Ok(txn)
    }

    pub fn start_auto_commit(&self) {
        self.auto_commit
            .store(true, std::sync::atomic::Ordering::Release);
        let mut self_txn = self.txn.lock().unwrap();
        if self_txn.is_some() || !self.can_edit() {
            return;
        }

        let txn = self.txn().unwrap();
        self_txn.replace(txn);
    }

    #[inline]
    pub fn renew_txn_if_auto_commit(&self, options: Option<CommitOptions>) {
        if self.auto_commit.load(std::sync::atomic::Ordering::Acquire) && self.can_edit() {
            let mut self_txn = self.txn.lock().unwrap();
            if self_txn.is_some() {
                return;
            }

            let mut txn = self.txn().unwrap();
            if let Some(options) = options {
                txn.set_options(options);
            }
            self_txn.replace(txn);
        }
    }
}

pub(crate) type OnCommitFn =
    Box<dyn FnOnce(&Arc<LoroMutex<DocState>>, &Arc<LoroMutex<OpLog>>, IdSpan) + Sync + Send>;

pub struct Transaction {
    peer: PeerID,
    origin: InternalString,
    start_counter: Counter,
    next_counter: Counter,
    start_lamport: Lamport,
    next_lamport: Lamport,
    doc: Weak<LoroDocInner>,
    frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>, // TODO: use a more efficient data structure
    event_hints: Vec<EventHintWithContainer>,
    pub(super) arena: SharedArena,
    finished: bool,
    on_commit: Option<OnCommitFn>,
    timestamp: Option<Timestamp>,
    msg: Option<Arc<str>>,
    latest_timestamp: Timestamp,
    pub(super) is_peer_first_appearance: bool,
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("peer", &self.peer)
            .field("origin", &self.origin)
            .field("start_counter", &self.start_counter)
            .field("next_counter", &self.next_counter)
            .field("start_lamport", &self.start_lamport)
            .field("next_lamport", &self.next_lamport)
            .field("frontiers", &self.frontiers)
            .field("local_ops", &self.local_ops)
            .field("event_hints", &self.event_hints)
            .field("arena", &self.arena)
            .field("finished", &self.finished)
            .field("on_commit", &self.on_commit.is_some())
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

/// Wrapper for EventHint that includes the container index to prevent cross-container merging
#[derive(Debug, Clone)]
pub(super) struct EventHintWithContainer {
    pub container_idx: ContainerIdx,
    pub hint: EventHint,
}

/// We can infer local events directly from the local behavior. This enum is used to
/// record them, so that we can avoid recalculate them when we commit the transaction.
///
/// For example, when we insert a text in wasm, users use the utf16 index to send the
/// command. However, internally loro will convert it to unicode index. But the users
/// still need events that are in utf16 index. To avoid the round trip, we record the
/// events here.
#[derive(Debug, Clone, EnumAsInner)]
pub(super) enum EventHint {
    Mark {
        start: u32,
        end: u32,
        style: Style,
    },
    InsertText {
        /// pos is a Unicode index. If wasm, it's a UTF-16 index.
        pos: u32,
        event_len: u32,
        unicode_len: u32,
        styles: StyleMeta,
    },
    /// pos is a Unicode index. If wasm, it's a UTF-16 index.
    DeleteText {
        span: DeleteSpan,
        unicode_len: usize,
    },
    InsertList {
        len: u32,
        pos: usize,
    },
    SetList {
        index: usize,
        value: LoroValue,
    },
    Move {
        value: LoroValue,
        from: u32,
        to: u32,
    },
    DeleteList(DeleteSpan),
    Map {
        key: InternalString,
        value: Option<LoroValue>,
    },
    // use vec because we could bring back some node that has children
    Tree(SmallVec<[TreeDiffItem; 1]>),
    MarkEnd,
    #[cfg(feature = "counter")]
    Counter(f64),
}

impl generic_btree::rle::HasLength for EventHint {
    fn rle_len(&self) -> usize {
        match self {
            EventHint::Mark { .. } => 1,
            EventHint::InsertText {
                unicode_len: len, ..
            } => *len as usize,
            EventHint::DeleteText { unicode_len, .. } => *unicode_len,
            EventHint::InsertList { len, .. } => *len as usize,
            EventHint::DeleteList(d) => d.len(),
            EventHint::Map { .. } => 1,
            EventHint::Tree(_) => 1,
            EventHint::MarkEnd => 1,
            EventHint::Move { .. } => 1,
            EventHint::SetList { .. } => 1,
            #[cfg(feature = "counter")]
            EventHint::Counter(_) => 1,
        }
    }
}

impl generic_btree::rle::Mergeable for EventHint {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (
                EventHint::InsertText {
                    pos,
                    unicode_len: _,
                    event_len,
                    styles,
                },
                EventHint::InsertText {
                    pos: r_pos,
                    styles: r_styles,
                    ..
                },
            ) => *pos + *event_len == *r_pos && styles == r_styles,
            (EventHint::InsertList { pos, len }, EventHint::InsertList { pos: pos_right, .. }) => {
                pos + *len as usize == *pos_right
            }
            // We don't merge delete text because it's hard to infer the correct pos to split:
            // `range` param is in unicode range, but the delete text event is in UTF-16 range.
            // Without the original text, it's impossible to convert the range.
            (EventHint::DeleteText { span, .. }, EventHint::DeleteText { span: r, .. }) => {
                span.is_mergable(r, &())
            }
            (EventHint::DeleteList(l), EventHint::DeleteList(r)) => l.is_mergable(r, &()),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (
                EventHint::InsertText {
                    event_len,
                    unicode_len: len,
                    ..
                },
                EventHint::InsertText {
                    event_len: r_event_len,
                    unicode_len: r_len,
                    ..
                },
            ) => {
                *len += *r_len;
                *event_len += *r_event_len;
            }
            (
                EventHint::InsertList { len, pos: _ },
                EventHint::InsertList { len: r_len, pos: _ },
            ) => *len += *r_len,
            (EventHint::DeleteList(l), EventHint::DeleteList(r)) => l.merge(r, &()),
            (
                EventHint::DeleteText { span, unicode_len },
                EventHint::DeleteText {
                    span: r_span,
                    unicode_len: r_len,
                },
            ) => {
                *unicode_len += *r_len;
                span.merge(r_span, &());
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, _: &Self) {
        unreachable!()
    }
}

impl Transaction {
    #[inline]
    pub fn new(doc: Arc<LoroDocInner>) -> Self {
        Self::new_with_origin(doc.clone(), "".into())
    }

    pub fn new_with_origin(doc: Arc<LoroDocInner>, origin: InternalString) -> Self {
        let oplog_lock = doc.oplog.lock().unwrap();
        let mut state_lock = doc.state.lock().unwrap();
        if state_lock.is_in_txn() {
            panic!("Cannot start a transaction while another one is in progress");
        }

        state_lock.start_txn(origin, crate::event::EventTriggerKind::Local);
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        let peer = state_lock.peer.load(std::sync::atomic::Ordering::Relaxed);
        let next_counter = oplog_lock.next_id(peer).counter;
        let next_lamport = oplog_lock.dag.frontiers_to_next_lamport(&frontiers);
        let latest_timestamp = oplog_lock.get_greatest_timestamp(&frontiers);
        oplog_lock
            .check_change_greater_than_last_peer_id(peer, next_counter, &frontiers)
            .unwrap();
        drop(state_lock);
        drop(oplog_lock);
        Self {
            peer,
            doc: Arc::downgrade(&doc),
            arena,
            frontiers,
            timestamp: None,
            next_counter,
            next_lamport,
            origin: Default::default(),
            start_counter: next_counter,
            start_lamport: next_lamport,
            event_hints: Default::default(),
            local_ops: RleVec::new(),
            finished: false,
            on_commit: None,
            msg: None,
            latest_timestamp,
            is_peer_first_appearance: false,
        }
    }

    pub fn set_origin(&mut self, origin: InternalString) {
        self.origin = origin;
    }

    pub fn set_timestamp(&mut self, time: Timestamp) {
        self.timestamp = Some(time);
    }

    pub fn set_msg(&mut self, msg: Option<Arc<str>>) {
        self.msg = msg;
    }

    pub fn local_ops(&self) -> &RleVec<[Op; 1]> {
        &self.local_ops
    }

    pub fn peer(&self) -> &PeerID {
        &self.peer
    }

    pub fn timestamp(&self) -> &Option<Timestamp> {
        &self.timestamp
    }

    pub fn frontiers(&self) -> &Frontiers {
        &self.frontiers
    }
    pub fn msg(&self) -> &Option<Arc<str>> {
        &self.msg
    }

    pub fn lamport(&self) -> &Lamport {
        &self.start_lamport
    }

    pub(crate) fn set_on_commit(&mut self, f: OnCommitFn) {
        self.on_commit = Some(f);
    }

    pub(crate) fn take_on_commit(&mut self) -> Option<OnCommitFn> {
        self.on_commit.take()
    }

    pub fn commit(mut self) -> Result<Option<CommitOptions>, LoroError> {
        self._commit()
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn _commit(&mut self) -> Result<Option<CommitOptions>, LoroError> {
        if self.finished {
            return Ok(None);
        }

        let Some(doc) = self.doc.upgrade() else {
            return Ok(None);
        };
        self.finished = true;
        if self.local_ops.is_empty() {
            let mut state = doc.state.lock().unwrap();
            state.abort_txn();
            return Ok(Some(self.take_options()));
        }

        let ops = std::mem::take(&mut self.local_ops);
        let deps = take(&mut self.frontiers);
        let change = Change {
            lamport: self.start_lamport,
            ops,
            deps,
            id: ID::new(self.peer, self.start_counter),
            timestamp: self.latest_timestamp.max(
                self.timestamp
                    .unwrap_or_else(|| doc.oplog.lock().unwrap().get_timestamp_for_next_txn()),
            ),
            commit_msg: take(&mut self.msg),
        };

        let change_meta = ChangeMeta::from_change(&change);
        {
            // add change to uncommit field of oplog
            let mut oplog = doc.oplog.lock().unwrap();
            oplog.set_uncommitted_change(change);
        }

        let modifier = ChangeModifier::default();
        doc.pre_commit_subs.emit(
            &(),
            PreCommitCallbackPayload {
                change_meta,
                origin: self.origin.to_string(),
                modifier: modifier.clone(),
            },
        );

        let mut oplog = doc.oplog.lock().unwrap();
        let mut state = doc.state.lock().unwrap();

        let mut change = oplog.uncommitted_change.take().unwrap();
        modifier.modify_change(&mut change);
        let diff = if state.is_recording() {
            Some(change_to_diff(
                &change,
                doc.clone(),
                std::mem::take(&mut self.event_hints),
            ))
        } else {
            None
        };

        let last_id = change.id_last();
        if let Err(err) = oplog.import_local_change(change) {
            state.abort_txn();
            drop(state);
            drop(oplog);
            return Err(err);
        }

        state.commit_txn(
            Frontiers::from_id(last_id),
            diff.map(|arr| InternalDocDiff {
                by: crate::event::EventTriggerKind::Local,
                origin: self.origin.clone(),
                diff: Cow::Owned(
                    arr.into_iter()
                        .map(|x| InternalContainerDiff {
                            idx: x.idx,
                            bring_back: false,
                            diff: (x.diff.into()),
                            diff_mode: crate::diff_calc::DiffMode::Linear,
                        })
                        .collect(),
                ),
                new_version: Cow::Borrowed(oplog.frontiers()),
            }),
        );
        drop(state);
        drop(oplog);
        if let Some(on_commit) = self.on_commit.take() {
            assert!(!doc.txn.is_locked());
            on_commit(&doc.state.clone(), &doc.oplog.clone(), self.id_span());
        }
        Ok(None)
    }

    fn take_options(&self) -> CommitOptions {
        let mut options = CommitOptions::new();
        if !self.origin.is_empty() {
            options = options.origin(self.origin.as_str());
        }
        if let Some(msg) = self.msg.as_ref() {
            options = options.commit_msg(msg);
        }
        if let Some(timestamp) = self.timestamp {
            options = options.timestamp(timestamp);
        }
        options
    }

    pub(super) fn apply_local_op(
        &mut self,
        container: ContainerIdx,
        content: RawOpContent,
        event: EventHint,
        // check whether context and txn are referring to the same state context
        doc: &LoroDoc,
    ) -> LoroResult<()> {
        // TODO: need to check if the doc is the same
        let this_doc = self.doc.upgrade().unwrap();
        if Arc::as_ptr(&this_doc.state) != Arc::as_ptr(&doc.state) {
            return Err(LoroError::UnmatchedContext {
                expected: this_doc
                    .state
                    .lock()
                    .unwrap()
                    .peer
                    .load(std::sync::atomic::Ordering::Relaxed),
                found: doc
                    .state
                    .lock()
                    .unwrap()
                    .peer
                    .load(std::sync::atomic::Ordering::Relaxed),
            });
        }

        let len = content.content_len();
        assert!(len > 0);
        let raw_op = RawOp {
            id: ID {
                peer: self.peer,
                counter: self.next_counter,
            },
            lamport: self.next_lamport,
            container,
            content,
        };

        let mut oplog = doc.oplog.lock().unwrap();
        let mut state = doc.state.lock().unwrap();
        if state.is_deleted(container) {
            return Err(LoroError::ContainerDeleted {
                container: Box::new(state.arena.idx_to_id(container).unwrap()),
            });
        }

        let op = self.arena.convert_raw_op(&raw_op);
        state.apply_local_op(&raw_op, &op)?;
        {
            if !self.is_peer_first_appearance && !oplog.dag.latest_vv_contains_peer(self.peer) {
                self.is_peer_first_appearance = true;
            }
            // update version info
            let dep_id = Frontiers::from_id(ID::new(self.peer, self.next_counter - 1));
            let start_id = ID::new(self.peer, self.next_counter);
            self.next_counter += len as Counter;
            oplog.dag.update_version_on_new_local_op(
                if self.local_ops.is_empty() {
                    &self.frontiers
                } else {
                    &dep_id
                },
                start_id,
                self.next_lamport,
                len,
            );
            self.next_lamport += len as Lamport;
            // set frontiers to the last op id
            let last_id = start_id.inc(len as Counter - 1);
            state.frontiers = Frontiers::from_id(last_id);
        };
        drop(state);
        drop(oplog);
        debug_assert_eq!(
            event.rle_len(),
            op.atom_len(),
            "event:{:#?} \nop:{:#?}",
            &event,
            &op
        );

        let hint_with_container = EventHintWithContainer {
            container_idx: container,
            hint: event,
        };
        
        match self.event_hints.last_mut() {
            Some(last) if last.container_idx == container && last.hint.can_merge(&hint_with_container.hint) => {
                last.hint.merge_right(&hint_with_container.hint);
            }
            _ => {
                self.event_hints.push(hint_with_container);
            }
        }
        self.local_ops.push(op);
        Ok(())
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Text);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_text()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        let id = id.into_container_id(&self.arena, ContainerType::List);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_list()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Map);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_map()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Tree);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_tree()
            .unwrap()
    }
    pub fn next_id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.next_counter,
        }
    }

    #[inline]
    pub fn id_span(&self) -> IdSpan {
        IdSpan::new(self.peer, self.start_counter, self.next_counter)
    }

    pub fn next_idlp(&self) -> IdLp {
        IdLp {
            peer: self.peer,
            lamport: self.next_lamport,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.local_ops.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        (self.next_counter - self.start_counter) as usize
    }

    pub(crate) fn set_options(&mut self, options: CommitOptions) {
        self.origin = options.origin.unwrap_or_default();
        self.msg = options.commit_msg;
        self.timestamp = options.timestamp;
    }

    pub(crate) fn set_default_options(&mut self, default_options: crate::loro::CommitOptions) {
        if self.origin.is_empty() {
            self.origin = default_options.origin.unwrap_or_default();
        }
        if self.msg.is_none() {
            self.msg = default_options.commit_msg;
        }
        if self.timestamp.is_none() {
            self.timestamp = default_options.timestamp;
        }
    }
}

impl Drop for Transaction {
    #[tracing::instrument(level = "debug", skip(self))]
    fn drop(&mut self) {
        if !self.finished {
            // TODO: should we abort here or commit here?
            // what if commit fails?
            self._commit().unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TxnContainerDiff {
    pub(crate) idx: ContainerIdx,
    pub(crate) diff: Diff,
}

// PERF: could be compacter
fn change_to_diff(
    change: &Change,
    doc: Arc<LoroDocInner>,
    event_hints: Vec<EventHintWithContainer>,
) -> Vec<TxnContainerDiff> {
    let mut ans: Vec<TxnContainerDiff> = Vec::with_capacity(change.ops.len());
    let peer = change.id.peer;
    let mut lamport = change.lamport;
    let mut event_hint_iter = event_hints.into_iter();
    let mut o_hint = event_hint_iter.next();
    let mut op_iter = change.ops.iter();
    while let Some(op) = op_iter.next() {
        let Some(hint) = o_hint.as_mut() else {
            unreachable!()
        };

        let mut ops: SmallVec<[&Op; 1]> = smallvec![op];
        let hint = match op.atom_len().cmp(&hint.hint.rle_len()) {
            std::cmp::Ordering::Less => {
                let mut len = op.atom_len();
                while len < hint.hint.rle_len() {
                    let next = op_iter.next().unwrap();
                    len += next.atom_len();
                    ops.push(next);
                }
                assert!(len == hint.hint.rle_len());
                match event_hint_iter.next() {
                    Some(n) => o_hint.replace(n).unwrap().hint,
                    None => o_hint.take().unwrap().hint,
                }
            }
            std::cmp::Ordering::Equal => match event_hint_iter.next() {
                Some(n) => o_hint.replace(n).unwrap().hint,
                None => o_hint.take().unwrap().hint,
            },
            std::cmp::Ordering::Greater => {
                unreachable!("{:#?}", &op)
            }
        };

        match &hint {
            EventHint::InsertText { .. }
            | EventHint::InsertList { .. }
            | EventHint::DeleteText { .. }
            | EventHint::DeleteList(_) => {}
            _ => {
                assert_eq!(ops.len(), 1);
            }
        }
        match hint {
            EventHint::Mark { start, end, style } => {
                let mut meta = StyleMeta::default();
                meta.insert(
                    style.key.clone(),
                    StyleMetaItem {
                        lamport,
                        peer: change.id.peer,
                        value: style.data,
                    },
                );
                let diff = DeltaRopeBuilder::new()
                    .retain(start as usize, Default::default())
                    .retain(
                        (end - start) as usize,
                        meta.to_option_map().unwrap_or_default().into(),
                    )
                    .build();
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::Text(diff),
                });
            }
            EventHint::InsertText { styles, pos, .. } => {
                let mut delta: TextDiff = DeltaRopeBuilder::new()
                    .retain(pos as usize, Default::default())
                    .build();
                for op in ops.iter() {
                    let InnerListOp::InsertText { slice, .. } = op.content.as_list().unwrap()
                    else {
                        unreachable!()
                    };

                    delta.push_insert(
                        slice.clone().into(),
                        styles.to_option_map().unwrap_or_default().into(),
                    );
                }
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::Text(delta),
                })
            }
            EventHint::DeleteText {
                span,
                unicode_len: _,
                // we don't need to iter over ops here, because we already
                // know what the events should be
            } => ans.push(TxnContainerDiff {
                idx: op.container,
                diff: Diff::Text(
                    DeltaRopeBuilder::new()
                        .retain(span.start() as usize, Default::default())
                        .delete(span.len())
                        .build(),
                ),
            }),
            EventHint::InsertList { pos, .. } => {
                // We should use pos from event hint because index in op may
                // be using op index for the MovableList
                for op in ops.iter() {
                    let (range, _) = op.content.as_list().unwrap().as_insert().unwrap();
                    let values = doc
                        .arena
                        .get_values(range.to_range())
                        .into_iter()
                        .map(|v| ValueOrHandler::from_value(v, &doc));
                    ans.push(TxnContainerDiff {
                        idx: op.container,
                        diff: Diff::List(
                            DeltaRopeBuilder::new()
                                .retain(pos, Default::default())
                                .insert_many(values, Default::default())
                                .build(),
                        ),
                    })
                }
            }
            EventHint::DeleteList(s) => {
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::List(
                        DeltaRopeBuilder::new()
                            .retain(s.start() as usize, Default::default())
                            .delete(s.len())
                            .build(),
                    ),
                });
            }
            EventHint::Map { key, value } => ans.push(TxnContainerDiff {
                idx: op.container,
                diff: Diff::Map(ResolvedMapDelta::new().with_entry(
                    key,
                    ResolvedMapValue {
                        value: value.map(|v| ValueOrHandler::from_value(v, &doc)),
                        idlp: IdLp::new(peer, lamport),
                    },
                )),
            }),
            EventHint::Tree(tree_diff) => {
                let mut diff = TreeDiff::default();
                diff.diff.extend(tree_diff.into_iter());
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::Tree(diff),
                });
            }
            EventHint::Move { from, to, value } => {
                let mut a = DeltaRopeBuilder::new()
                    .retain(from as usize, Default::default())
                    .delete(1)
                    .build();
                a.compose(
                    &DeltaRopeBuilder::new()
                        .retain(to as usize, Default::default())
                        .insert(
                            ArrayVec::from([ValueOrHandler::from_value(value, &doc)]),
                            ListDeltaMeta { from_move: true },
                        )
                        .build(),
                );
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::List(a),
                });
            }
            EventHint::SetList { index, value } => {
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::List(
                        DeltaRopeBuilder::new()
                            .retain(index, Default::default())
                            .delete(1)
                            .insert(
                                ArrayVec::from([ValueOrHandler::from_value(value, &doc)]),
                                Default::default(),
                            )
                            .build(),
                    ),
                });
            }
            EventHint::MarkEnd => {
                // do nothing
            }
            #[cfg(feature = "counter")]
            EventHint::Counter(diff) => {
                ans.push(TxnContainerDiff {
                    idx: op.container,
                    diff: Diff::Counter(diff),
                });
            }
        }

        lamport += ops
            .iter()
            .map(|x| x.content_len() as Lamport)
            .sum::<Lamport>();
    }
    ans
}
