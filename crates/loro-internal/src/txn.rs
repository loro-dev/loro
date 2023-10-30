use std::{
    borrow::Cow,
    mem::take,
    sync::{Arc, Mutex, Weak},
};

use debug_log::debug_dbg;
use enum_as_inner::EnumAsInner;
use generic_btree::rle::{HasLength as RleHasLength, Mergeable, Sliceable as GBSliceable};
use loro_common::{ContainerType, LoroResult};
use rle::{HasLength, Mergable, RleVec, Sliceable};

use crate::{
    change::{get_sys_timestamp, Change, Lamport, Timestamp},
    container::{idx::ContainerIdx, list::list_op::DeleteSpan, richtext::Style, IntoContainerId},
    delta::{Delta, MapValue, TreeDelta, TreeDiff},
    event::Diff,
    id::{Counter, PeerID, ID},
    op::{Op, RawOp, RawOpContent},
    span::HasIdSpan,
    version::Frontiers,
    InternalString, LoroError, LoroValue,
};

use super::{
    arena::SharedArena,
    event::{InternalContainerDiff, InternalDocDiff},
    handler::{ListHandler, MapHandler, TextHandler, TreeHandler},
    oplog::OpLog,
    state::{DocState, State},
};

pub type OnCommitFn = Box<dyn FnOnce(&Arc<Mutex<DocState>>) + Sync + Send>;

pub struct Transaction {
    global_txn: Weak<Mutex<Option<Transaction>>>,
    peer: PeerID,
    origin: InternalString,
    start_counter: Counter,
    next_counter: Counter,
    start_lamport: Lamport,
    next_lamport: Lamport,
    state: Arc<Mutex<DocState>>,
    oplog: Arc<Mutex<OpLog>>,
    frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>, // TODO: use a more efficient data structure
    event_hints: Vec<EventHint>,
    pub(super) arena: SharedArena,
    finished: bool,
    on_commit: Option<OnCommitFn>,
    timestamp: Option<Timestamp>,
}

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
        len: u32,
        styles: Vec<Style>,
    },
    /// pos is a Unicode index. If wasm, it's a UTF-16 index.
    DeleteText(DeleteSpan),
    InsertList {
        pos: usize,
        value: LoroValue,
    },
    DeleteList(DeleteSpan),
    Map {
        key: InternalString,
        value: Option<LoroValue>,
    },
    Tree(TreeDiff),
    None,
}

impl generic_btree::rle::HasLength for EventHint {
    fn rle_len(&self) -> usize {
        match self {
            EventHint::Mark { .. } => 1,
            EventHint::InsertText { len, .. } => *len as usize,
            EventHint::DeleteText(d) => d.len(),
            EventHint::InsertList { .. } => 1,
            EventHint::DeleteList(d) => d.len(),
            EventHint::Map { .. } => 1,
            EventHint::Tree(_) => 1,
            EventHint::None => 1,
        }
    }
}

impl generic_btree::rle::Mergeable for EventHint {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (
                EventHint::InsertText { pos, len, styles },
                EventHint::InsertText {
                    pos: r_pos,
                    styles: r_styles,
                    ..
                },
            ) => *pos + *len == *r_pos && styles == r_styles,
            (EventHint::DeleteText(l), EventHint::DeleteText(r)) => l.is_mergable(r, &()),
            (EventHint::DeleteList(l), EventHint::DeleteList(r)) => l.is_mergable(r, &()),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (EventHint::InsertText { len, .. }, EventHint::InsertText { len: r_len, .. }) => {
                *len += *r_len;
            }
            (EventHint::DeleteText(l), EventHint::DeleteText(r)) => l.merge(r, &()),
            (EventHint::DeleteList(l), EventHint::DeleteList(r)) => l.merge(r, &()),
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, _: &Self) {
        unreachable!()
    }
}

impl generic_btree::rle::Sliceable for EventHint {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        match self {
            EventHint::InsertText {
                pos,
                len: _,
                styles,
            } => EventHint::InsertText {
                pos: *pos + range.start as u32,
                len: range.len() as u32,
                styles: styles.clone(),
            },
            EventHint::DeleteText(d) => EventHint::DeleteText(d.slice(range.start, range.end)),
            EventHint::DeleteList(d) => EventHint::DeleteList(d.slice(range.start, range.end)),
            a => {
                assert_eq!(a.rle_len(), range.len());
                a.clone()
            }
        }
    }
}

impl Transaction {
    pub fn new(
        state: Arc<Mutex<DocState>>,
        oplog: Arc<Mutex<OpLog>>,
        global_txn: Weak<Mutex<Option<Transaction>>>,
    ) -> Self {
        Self::new_with_origin(state, oplog, "".into(), global_txn)
    }

    pub fn new_with_origin(
        state: Arc<Mutex<DocState>>,
        oplog: Arc<Mutex<OpLog>>,
        origin: InternalString,
        global_txn: Weak<Mutex<Option<Transaction>>>,
    ) -> Self {
        let mut state_lock = state.lock().unwrap();
        if state_lock.is_in_txn() {
            panic!("Cannot start a transaction while another one is in progress");
        }

        let oplog_lock = oplog.lock().unwrap();
        state_lock.start_txn(origin, true);
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        let peer = state_lock.peer;
        let next_counter = oplog_lock.next_id(peer).counter;
        let next_lamport = oplog_lock.dag.frontiers_to_next_lamport(&frontiers);
        drop(state_lock);
        drop(oplog_lock);
        Self {
            global_txn,
            origin: Default::default(),
            peer,
            start_counter: next_counter,
            start_lamport: next_lamport,
            next_counter,
            state,
            arena,
            oplog,
            next_lamport,
            event_hints: Default::default(),
            frontiers,
            local_ops: RleVec::new(),
            finished: false,
            on_commit: None,
            timestamp: None,
        }
    }

    pub fn set_origin(&mut self, origin: InternalString) {
        self.origin = origin;
    }

    pub fn commit(mut self) -> Result<(), LoroError> {
        self._commit()
    }

    pub fn set_timestamp(&mut self, time: Timestamp) {
        self.timestamp = Some(time);
    }

    pub(crate) fn set_on_commit(&mut self, f: OnCommitFn) {
        self.on_commit = Some(f);
    }

    pub(crate) fn take_on_commit(&mut self) -> Option<OnCommitFn> {
        self.on_commit.take()
    }

    pub fn abort(mut self) {
        self._abort();
    }

    fn _abort(&mut self) {
        if self.finished {
            return;
        }

        self.finished = true;
        self.state.lock().unwrap().abort_txn();
        self.local_ops.clear();
        self.event_hints.clear();
    }

    fn _commit(&mut self) -> Result<(), LoroError> {
        if self.finished {
            return Ok(());
        }

        self.finished = true;
        let mut state = self.state.lock().unwrap();
        if self.local_ops.is_empty() {
            state.abort_txn();
            return Ok(());
        }

        let ops = std::mem::take(&mut self.local_ops);
        let mut oplog = self.oplog.lock().unwrap();
        let deps = take(&mut self.frontiers);
        let change = Change {
            lamport: self.start_lamport,
            ops,
            deps,
            id: ID::new(self.peer, self.start_counter),
            timestamp: oplog
                .latest_timestamp
                .max(self.timestamp.unwrap_or_else(get_sys_timestamp)),
        };

        let diff = if state.is_recording() {
            Some(change_to_diff(
                &change,
                &oplog.arena,
                std::mem::take(&mut self.event_hints),
            ))
        } else {
            None
        };

        let last_id = change.id_last();
        if let Err(err) = oplog.import_local_change(change) {
            drop(state);
            drop(oplog);
            self._abort();
            return Err(err);
        }

        state.commit_txn(
            Frontiers::from_id(last_id),
            diff.map(|arr| InternalDocDiff {
                local: true,
                origin: self.origin.clone(),
                diff: Cow::Owned(
                    arr.into_iter()
                        .map(|x| InternalContainerDiff {
                            idx: x.idx,
                            reset: false,
                            is_container_deleted: false,
                            diff: x.diff.into(),
                        })
                        .collect(),
                ),
                new_version: Cow::Borrowed(oplog.frontiers()),
            }),
        );
        drop(state);
        drop(oplog);
        if let Some(on_commit) = self.on_commit.take() {
            on_commit(&self.state);
        }
        Ok(())
    }

    pub(super) fn apply_local_op(
        &mut self,
        container: ContainerIdx,
        content: RawOpContent,
        event: EventHint,
        // check whther context and txn are refering to the same state context
        state_ref: &Weak<Mutex<DocState>>,
    ) -> LoroResult<()> {
        if Arc::as_ptr(&self.state) != Weak::as_ptr(state_ref) {
            return Err(LoroError::UnmatchedContext {
                expected: self.state.lock().unwrap().peer,
                found: state_ref.upgrade().unwrap().lock().unwrap().peer,
            });
        }

        let len = content.content_len();
        let raw_op = RawOp {
            id: ID {
                peer: self.peer,
                counter: self.next_counter,
            },
            lamport: self.next_lamport,
            container,
            content,
        };

        let mut state = self.state.lock().unwrap();
        let op = self.arena.convert_raw_op(&raw_op);
        state.apply_local_op(&raw_op, &op)?;
        drop(state);
        assert_eq!(event.rle_len(), op.atom_len());
        match self.event_hints.last_mut() {
            Some(last) if last.can_merge(&event) => {
                last.merge_right(&event);
            }
            _ => {
                self.event_hints.push(event);
            }
        }
        self.local_ops.push(op);
        self.next_counter += len as Counter;
        self.next_lamport += len as Lamport;
        Ok(())
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        let idx = self.get_container_idx(id, ContainerType::Text);
        TextHandler::new(self.global_txn.clone(), idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        let idx = self.get_container_idx(id, ContainerType::List);
        ListHandler::new(self.global_txn.clone(), idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        let idx = self.get_container_idx(id, ContainerType::Map);
        MapHandler::new(self.global_txn.clone(), idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        let idx = self.get_container_idx(id, ContainerType::Tree);
        TreeHandler::new(self.global_txn.clone(), idx, Arc::downgrade(&self.state))
    }

    fn get_container_idx<I: IntoContainerId>(&self, id: I, c_type: ContainerType) -> ContainerIdx {
        let id = id.into_container_id(&self.arena, c_type);
        self.arena.register_container(&id)
    }

    pub fn get_value_by_idx(&self, idx: ContainerIdx) -> LoroValue {
        self.state.lock().unwrap().get_value_by_idx(idx)
    }

    #[allow(unused)]
    pub(crate) fn with_state<F, R>(&self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&State) -> R,
    {
        let state = self.state.lock().unwrap();
        f(state.get_state(idx).unwrap())
    }

    pub fn next_id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.next_counter,
        }
    }
}

impl Drop for Transaction {
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
    _arena: &SharedArena,
    event_hints: Vec<EventHint>,
) -> Vec<TxnContainerDiff> {
    let mut ans: Vec<TxnContainerDiff> = Vec::with_capacity(change.ops.len());
    let peer = change.id.peer;
    let mut lamport = change.lamport;
    let mut event_hint_iter = event_hints.into_iter();
    let mut o_hint = event_hint_iter.next();

    for op in change.ops.iter() {
        let Some(hint) = o_hint.as_mut() else {
            unreachable!()
        };

        let hint = match op.atom_len().cmp(&hint.rle_len()) {
            std::cmp::Ordering::Less => {
                let ans = hint.slice(..op.atom_len());
                hint.slice_(op.atom_len()..);
                ans
            }
            std::cmp::Ordering::Equal => match event_hint_iter.next() {
                Some(n) => o_hint.replace(n).unwrap(),
                None => o_hint.take().unwrap(),
            },
            std::cmp::Ordering::Greater => {
                unreachable!()
            }
        };

        'outer: {
            let diff: Diff =
                match hint {
                    EventHint::Mark { start, end, style } => {
                        Diff::Text(Delta::new().retain(start as usize).retain_with_meta(
                            (end - start) as usize,
                            crate::delta::StyleMeta { vec: vec![style] },
                        ))
                    }
                    EventHint::InsertText { pos, styles, .. } => {
                        let slice = op.content.as_list().unwrap().as_insert_text().unwrap().0;
                        Diff::Text(Delta::new().retain(pos as usize).insert_with_meta(
                            slice.clone(),
                            crate::delta::StyleMeta { vec: styles },
                        ))
                    }
                    EventHint::DeleteText(s) => {
                        Diff::Text(Delta::new().retain(s.start() as usize).delete(s.len()))
                    }
                    EventHint::InsertList { pos, value } => {
                        Diff::List(Delta::new().retain(pos).insert(vec![value]))
                    }
                    EventHint::DeleteList(s) => {
                        Diff::List(Delta::new().retain(s.start() as usize).delete(s.len()))
                    }
                    EventHint::Map { key, value } => {
                        Diff::NewMap(crate::delta::MapDelta::new().with_entry(
                            key,
                            MapValue {
                                counter: op.counter,
                                value,
                                lamport: (lamport, peer),
                            },
                        ))
                    }
                    EventHint::Tree(tree_diff) => Diff::Tree(TreeDelta::default().push(tree_diff)),
                    EventHint::None => {
                        // do nothing
                        break 'outer;
                    }
                };

            ans.push(TxnContainerDiff {
                idx: op.container,
                diff,
            });
        }

        lamport += op.content_len() as Lamport;
    }

    ans
}
