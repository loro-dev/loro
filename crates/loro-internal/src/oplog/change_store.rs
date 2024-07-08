use block_encode::decode_block_range;
use bytes::Bytes;
use itertools::Itertools;
use loro_common::{
    Counter, HasCounterSpan, HasId, HasIdSpan, IdLp, IdSpan, Lamport, LoroError, LoroResult,
    PeerID, ID,
};
use once_cell::sync::OnceCell;
use rle::{HasLength, Mergable};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, VecDeque},
    ops::{Bound, Deref},
    sync::{atomic::AtomicI64, Arc, Mutex},
};
mod block_encode;
mod delta_rle_encode;
use crate::{
    arena::SharedArena, change::Change, estimated_size::EstimatedSize, kv_store::KvStore, op::Op,
    version::Frontiers, VersionVector,
};

use self::block_encode::{decode_block, decode_header, encode_block, ChangesBlockHeader};

const MAX_BLOCK_SIZE: usize = 1024 * 4;

/// # Invariances
///
/// - We don't allow holes in a block or between two blocks with the same peer id.
///   The [Change] should be continuous for each peer.
/// - However, the first block of a peer can have counter > 0 so that we can trim the history.
#[derive(Debug, Clone)]
pub struct ChangeStore {
    arena: SharedArena,
    /// max known version vector
    vv: VersionVector,
    /// The start version vector of the first block for each peer.
    /// It allows us to trim the history
    start_vv: VersionVector,
    /// The last version of the trimmed history.
    start_frontiers: Frontiers,
    /// It's more like a parsed cache for binary_kv.
    mem_parsed_kv: Arc<Mutex<BTreeMap<ID, Arc<ChangesBlock>>>>,
    external_kv: Arc<Mutex<dyn KvStore>>,
    merge_interval: Arc<AtomicI64>,
}

impl ChangeStore {
    pub fn new_mem(a: &SharedArena, merge_interval: Arc<AtomicI64>) -> Self {
        Self {
            merge_interval,
            vv: VersionVector::new(),
            arena: a.clone(),
            start_vv: VersionVector::new(),
            start_frontiers: Frontiers::default(),
            mem_parsed_kv: Arc::new(Mutex::new(BTreeMap::new())),
            external_kv: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn insert_change(&mut self, mut change: Change) {
        let id = change.id;
        let mut kv = self.mem_parsed_kv.lock().unwrap();
        if let Some(old) = self.vv.get_mut(&change.id.peer) {
            if *old == change.id.counter {
                *old = change.id.counter + change.atom_len() as Counter;
            } else {
                todo!(
                    "{} != {}, FIXME: update start_vv and start_frontiers if needed",
                    old,
                    change.id.counter
                );
            }
        } else {
            self.vv.insert(
                change.id.peer,
                change.id.counter + change.atom_len() as Counter,
            );
        }

        if let Some((_id, block)) = kv.range_mut(..id).next_back() {
            if block.peer == change.id.peer {
                if block.counter_range.1 != change.id.counter {
                    panic!("counter should be continuous")
                }

                match block.push_change(
                    change,
                    self.merge_interval
                        .load(std::sync::atomic::Ordering::Acquire),
                ) {
                    Ok(_) => {
                        return;
                    }
                    Err(c) => change = c,
                }
            }
        }

        kv.insert(id, Arc::new(ChangesBlock::new(change, &self.arena)));
    }

    pub fn block_num(&self) -> usize {
        let kv = self.mem_parsed_kv.lock().unwrap();
        kv.len()
    }

    /// Flush the cached change to kv_store
    fn flush(&mut self) {
        let mut mem = self.mem_parsed_kv.lock().unwrap();
        let mut store = self.external_kv.lock().unwrap();
        for (id, block) in mem.iter_mut() {
            if !block.flushed {
                let bytes = block.bytes(&self.arena);
                Arc::make_mut(block).flushed = true;
                store.set(&id.to_bytes(), bytes.bytes);
            }
        }

        loop {
            let old_vv_bytes = store.get(b"vv");
            let new_vv = old_vv_bytes
                .as_ref()
                .map(|bytes| VersionVector::decode(bytes).unwrap())
                .unwrap_or_default();
            self.vv.merge(&new_vv);
            let vv_bytes = self.vv.encode();
            if store.compare_and_swap(b"vv", old_vv_bytes, vv_bytes.into()) {
                break;
            }
        }
    }

    pub(crate) fn encode_all(&mut self) -> Bytes {
        self.flush();
        self.external_kv.lock().unwrap().export_all()
    }

    pub(crate) fn decode_all(&self, blocks: Bytes) -> Result<(), LoroError> {
        self.external_kv
            .lock()
            .unwrap()
            .import_all(blocks)
            .map_err(|e| LoroError::DecodeError(e.into_boxed_str()))
        // todo!("replace with kv store");
        // let mut kv = self.mem_kv.lock().unwrap();
        // assert!(kv.is_empty());
        // let mut reader = blocks;
        // while !reader.is_empty() {
        //     let size = leb128::read::unsigned(&mut reader).unwrap();
        //     let block_bytes = &reader[0..size as usize];
        //     let block = ChangesBlock::from_bytes(Bytes::copy_from_slice(block_bytes))?;
        //     kv.insert(block.id(), Arc::new(block));
        //     reader = &reader[size as usize..];
        // }
        // Ok(())
    }

    fn get_parsed_block(&self, id: ID) -> Option<Arc<ChangesBlock>> {
        let mut kv = self.mem_parsed_kv.lock().unwrap();
        let (_id, block) = kv.range_mut(..=id).next_back()?;
        if block.peer == id.peer && block.counter_range.1 > id.counter {
            block.ensure_changes().expect("Parse block error");
            return Some(block.clone());
        }

        let store = self.external_kv.lock().unwrap();
        let mut iter = store.scan(
            std::ops::Bound::Unbounded,
            Bound::Included(&Bytes::copy_from_slice(&id.to_bytes())),
        );
        let (b_id, b_bytes) = iter.next_back()?;
        let block_id: ID = ID::from_bytes(&b_id[..]);
        let block = ChangesBlock::from_bytes(b_bytes, true).unwrap();
        if block_id.peer == id.peer
            && block_id.counter <= id.counter
            && block.counter_range.1 > id.counter
        {
            let mut arc_block = Arc::new(block);
            arc_block.ensure_changes().expect("Parse block error");
            kv.insert(block_id, arc_block.clone());
            return Some(arc_block);
        }

        None
    }

    pub fn get_change(&self, id: ID) -> Option<BlockChangeRef> {
        let block = self.get_parsed_block(id)?;
        Some(BlockChangeRef {
            change_index: block.get_change_index_by_counter(id.counter).unwrap(),
            block: block.clone(),
        })
    }

    /// Get the change with the given peer and lamport.
    ///
    /// If not found, return the change with the greatest lamport that is smaller than the given lamport.
    pub fn get_change_by_idlp(&self, idlp: IdLp) -> Option<BlockChangeRef> {
        let mut kv = self.mem_parsed_kv.lock().unwrap();
        let mut iter = kv.range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, i32::MAX));
        while let Some((_id, block)) = iter.next_back() {
            if block.lamport_range.0 <= idlp.lamport {
                block.ensure_changes().unwrap();
                let index = block.get_change_index_by_lamport(idlp.lamport)?;
                return Some(BlockChangeRef {
                    change_index: index,
                    block: block.clone(),
                });
            }
        }

        let (id, bytes) = self.external_kv.lock().unwrap().binary_search_by(
            Bound::Included(&ID::new(idlp.peer, 0).to_bytes()),
            Bound::Excluded(&ID::new(idlp.peer, Counter::MAX).to_bytes()),
            Box::new(move |_k, v| {
                let mut b = ChangesBlockBytes::new(v.clone());
                let range = b.lamport_range();
                if range.0 <= idlp.lamport && range.1 > idlp.lamport {
                    Ordering::Equal
                } else if range.1 <= idlp.lamport {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }),
        )?;

        let block_id = ID::from_bytes(&id);
        let mut block = Arc::new(ChangesBlock::from_bytes(bytes, true).unwrap());
        block.ensure_changes().unwrap();
        kv.insert(block_id, block.clone());
        let index = block.get_change_index_by_lamport(idlp.lamport)?;
        Some(BlockChangeRef {
            change_index: index,
            block,
        })
    }

    pub fn visit_all_changes(&self, f: &mut dyn FnMut(&Change)) {
        self.ensure_block_parsed_in_range(Bound::Unbounded, Bound::Unbounded);
        let mut mem_kv = self.mem_parsed_kv.lock().unwrap();
        for (_, block) in mem_kv.iter_mut() {
            block.ensure_changes().unwrap();
            for c in block.content.try_changes().unwrap() {
                f(c);
            }
        }
    }

    fn ensure_block_parsed_in_range(&self, start: Bound<&[u8]>, end: Bound<&[u8]>) {
        let kv = self.external_kv.lock().unwrap();
        let mut mem_kv = self.mem_parsed_kv.lock().unwrap();
        for (id, bytes) in kv.scan(start, end) {
            let id = ID::from_bytes(&id);
            if mem_kv.contains_key(&id) {
                continue;
            }

            let block = ChangesBlock::from_bytes(bytes.clone(), true).unwrap();
            mem_kv.insert(id, Arc::new(block));
        }
    }

    pub fn iter_changes(&self, id_span: IdSpan) -> impl Iterator<Item = BlockChangeRef> + '_ {
        self.ensure_block_parsed_in_range(
            Bound::Included(&id_span.id_start().to_bytes()),
            Bound::Excluded(&id_span.id_end().to_bytes()),
        );
        let mut kv = self.mem_parsed_kv.lock().unwrap();
        let start_counter = kv
            .range(..=id_span.id_start())
            .next_back()
            .map(|(id, _)| id.counter)
            .unwrap_or(0);
        let iter = kv
            .range_mut(
                ID::new(id_span.peer, start_counter)..ID::new(id_span.peer, id_span.counter.end),
            )
            .filter_map(|(_id, block)| {
                if block.counter_range.1 < id_span.counter.start {
                    return None;
                }

                block.ensure_changes().unwrap();
                Some(block.clone())
            })
            // TODO: PERF avoid alloc
            .collect_vec();

        iter.into_iter().flat_map(move |block| {
            let changes = block.content.try_changes().unwrap();
            let start;
            let end;
            if id_span.counter.start <= block.counter_range.0
                && id_span.counter.end >= block.counter_range.1
            {
                start = 0;
                end = changes.len().saturating_sub(1);
            } else {
                start = block
                    .get_change_index_by_counter(id_span.counter.start)
                    .unwrap_or(0);
                end = block
                    .get_change_index_by_counter(id_span.counter.end)
                    .unwrap_or(changes.len().saturating_sub(1));
            }

            (start..=end).map(move |i| BlockChangeRef {
                change_index: i,
                block: block.clone(),
            })
        })
    }

    pub(crate) fn get_blocks_in_range(&self, id_span: IdSpan) -> VecDeque<Arc<ChangesBlock>> {
        let mut kv = self.mem_parsed_kv.lock().unwrap();
        let start_counter = kv
            .range(..=id_span.id_start())
            .next_back()
            .map(|(id, _)| id.counter)
            .unwrap_or(0);
        let vec = kv
            .range_mut(
                ID::new(id_span.peer, start_counter)..ID::new(id_span.peer, id_span.counter.end),
            )
            .filter_map(|(_id, block)| {
                if block.counter_range.1 < id_span.counter.start {
                    return None;
                }

                block.ensure_changes().unwrap();
                Some(block.clone())
            })
            // TODO: PERF avoid alloc
            .collect();
        vec
    }

    pub fn change_num(&self) -> usize {
        let mut kv = self.mem_parsed_kv.lock().unwrap();
        kv.iter_mut().map(|(_, block)| block.change_num()).sum()
    }
}

#[derive(Clone, Debug)]
pub struct BlockChangeRef {
    pub block: Arc<ChangesBlock>,
    pub change_index: usize,
}

impl Deref for BlockChangeRef {
    type Target = Change;
    fn deref(&self) -> &Change {
        &self.block.content.try_changes().unwrap()[self.change_index]
    }
}

impl BlockChangeRef {
    pub fn get_op_with_counter(&self, counter: Counter) -> Option<BlockOpRef> {
        if counter >= self.ctr_end() {
            return None;
        }

        let index = self.ops.search_atom_index(counter);
        Some(BlockOpRef {
            block: self.block.clone(),
            change_index: self.change_index,
            op_index: index,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BlockOpRef {
    pub block: Arc<ChangesBlock>,
    pub change_index: usize,
    pub op_index: usize,
}

impl Deref for BlockOpRef {
    type Target = Op;

    fn deref(&self) -> &Op {
        &self.block.content.try_changes().unwrap()[self.change_index].ops[self.op_index]
    }
}

#[derive(Debug, Clone)]
pub struct ChangesBlock {
    peer: PeerID,
    counter_range: (Counter, Counter),
    lamport_range: (Lamport, Lamport),
    /// Estimated size of the block in bytes
    estimated_size: usize,
    flushed: bool,
    content: ChangesBlockContent,
}

impl ChangesBlock {
    pub fn from_bytes(bytes: Bytes, flushed: bool) -> LoroResult<Self> {
        let len = bytes.len();
        let mut bytes = ChangesBlockBytes::new(bytes);
        let peer = bytes.peer();
        let counter_range = bytes.counter_range();
        let lamport_range = bytes.lamport_range();
        let content = ChangesBlockContent::Bytes(bytes);
        Ok(Self {
            peer,
            estimated_size: len,
            counter_range,
            lamport_range,
            flushed,
            content,
        })
    }

    pub(crate) fn content(&self) -> &ChangesBlockContent {
        &self.content
    }

    pub fn new(change: Change, a: &SharedArena) -> Self {
        let atom_len = change.atom_len();
        let counter_range = (change.id.counter, change.id.counter + atom_len as Counter);
        let lamport_range = (change.lamport, change.lamport + atom_len as Lamport);
        let estimated_size = change.estimate_storage_size();
        let peer = change.id.peer;
        let content = ChangesBlockContent::Changes(Arc::new(vec![change]));
        Self {
            peer,
            counter_range,
            lamport_range,
            estimated_size,
            content,
            flushed: false,
        }
    }

    pub fn cmp_id(&self, id: ID) -> Ordering {
        self.peer.cmp(&id.peer).then_with(|| {
            if self.counter_range.0 > id.counter {
                Ordering::Greater
            } else if self.counter_range.1 <= id.counter {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    pub fn cmp_idlp(&self, idlp: (PeerID, Lamport)) -> Ordering {
        self.peer.cmp(&idlp.0).then_with(|| {
            if self.lamport_range.0 > idlp.1 {
                Ordering::Greater
            } else if self.lamport_range.1 <= idlp.1 {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    fn is_full(&self) -> bool {
        self.estimated_size > MAX_BLOCK_SIZE
    }

    pub fn push_change(
        self: &mut Arc<Self>,
        change: Change,
        merge_interval: i64,
    ) -> Result<(), Change> {
        if self.counter_range.1 != change.id.counter {
            return Err(change);
        }

        let atom_len = change.atom_len();
        let next_lamport = change.lamport + atom_len as Lamport;
        let next_counter = change.id.counter + atom_len as Counter;

        let is_full = self.is_full();
        let this = Arc::make_mut(self);
        let changes = this.content.changes_mut().unwrap();
        let changes = Arc::make_mut(changes);
        match changes.last_mut() {
            Some(last)
                if change.deps_on_self()
                    && change.timestamp - last.timestamp < merge_interval
                    && (!is_full
                        || (change.ops.len() == 1
                            && last.ops.last().unwrap().is_mergable(&change.ops[0], &()))) =>
            {
                for op in change.ops.into_iter() {
                    let size = op.estimate_storage_size();
                    if !last.ops.push(op) {
                        this.estimated_size += size;
                    }
                }
            }
            _ => {
                if is_full {
                    return Err(change);
                } else {
                    this.estimated_size += change.estimate_storage_size();
                    changes.push(change);
                }
            }
        }

        this.counter_range.1 = next_counter;
        this.lamport_range.1 = next_lamport;
        Ok(())
    }

    pub fn bytes<'a>(self: &'a mut Arc<Self>, a: &SharedArena) -> ChangesBlockBytes {
        match &self.content {
            ChangesBlockContent::Bytes(bytes) => bytes.clone(),
            ChangesBlockContent::Both(_, bytes) => bytes.clone(),
            ChangesBlockContent::Changes(changes) => {
                let bytes = ChangesBlockBytes::serialize(changes, a);
                let c = Arc::clone(changes);
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Both(c, bytes.clone());
                bytes
            }
        }
    }

    pub fn ensure_changes(self: &mut Arc<Self>) -> LoroResult<()> {
        match &self.content {
            ChangesBlockContent::Changes(_) => Ok(()),
            ChangesBlockContent::Both(_, _) => Ok(()),
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(&SharedArena::new())?;
                let b = bytes.clone();
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Both(Arc::new(changes), b);
                Ok(())
            }
        }
    }

    fn get_change_index_by_counter(&self, counter: Counter) -> Option<usize> {
        let changes = self.content.try_changes().unwrap();
        let r = changes.binary_search_by(|c| {
            if c.id.counter > counter {
                Ordering::Greater
            } else if (c.id.counter + c.content_len() as Counter) <= counter {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });

        match r {
            Ok(found) => Some(found),
            Err(_) => None,
        }
    }

    fn get_change_index_by_lamport(&self, lamport: Lamport) -> Option<usize> {
        let changes = self.content.try_changes().unwrap();
        let r = changes.binary_search_by(|c| {
            if c.lamport > lamport {
                Ordering::Greater
            } else if (c.lamport + c.content_len() as Lamport) <= lamport {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });

        match r {
            Ok(found) => Some(found),
            Err(not_found) => {
                if not_found == 0 {
                    None
                } else {
                    Some(not_found - 1)
                }
            }
        }
    }

    fn get_changes(&mut self) -> LoroResult<&Vec<Change>> {
        self.content.changes()
    }

    fn id(&self) -> ID {
        ID::new(self.peer, self.counter_range.0)
    }

    pub fn change_num(&self) -> usize {
        match &self.content {
            ChangesBlockContent::Changes(c) => c.len(),
            ChangesBlockContent::Bytes(b) => b.len_changes(),
            ChangesBlockContent::Both(c, _) => c.len(),
        }
    }
}

#[derive(Clone)]
pub(crate) enum ChangesBlockContent {
    Changes(Arc<Vec<Change>>),
    Bytes(ChangesBlockBytes),
    Both(Arc<Vec<Change>>, ChangesBlockBytes),
}

impl ChangesBlockContent {
    pub fn changes(&mut self) -> LoroResult<&Vec<Change>> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => Ok(changes),
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(&SharedArena::new())?;
                *self = ChangesBlockContent::Both(Arc::new(changes), bytes.clone());
                self.changes()
            }
        }
    }

    /// Note that this method will invalidate the stored bytes
    fn changes_mut(&mut self) -> LoroResult<&mut Arc<Vec<Change>>> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => {
                *self = ChangesBlockContent::Changes(std::mem::take(changes));
                self.changes_mut()
            }
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(&SharedArena::new())?;
                *self = ChangesBlockContent::Changes(Arc::new(changes));
                self.changes_mut()
            }
        }
    }

    pub(crate) fn try_changes(&self) -> Option<&Vec<Change>> {
        match self {
            ChangesBlockContent::Changes(changes) => Some(changes),
            ChangesBlockContent::Both(changes, _) => Some(changes),
            ChangesBlockContent::Bytes(_) => None,
        }
    }

    pub(crate) fn len_changes(&self) -> usize {
        match self {
            ChangesBlockContent::Changes(changes) => changes.len(),
            ChangesBlockContent::Both(changes, _) => changes.len(),
            ChangesBlockContent::Bytes(bytes) => bytes.len_changes(),
        }
    }
}

impl std::fmt::Debug for ChangesBlockContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangesBlockContent::Changes(changes) => f
                .debug_tuple("ChangesBlockContent::Changes")
                .field(changes)
                .finish(),
            ChangesBlockContent::Bytes(_bytes) => {
                f.debug_tuple("ChangesBlockContent::Bytes").finish()
            }
            ChangesBlockContent::Both(changes, _bytes) => f
                .debug_tuple("ChangesBlockContent::Both")
                .field(changes)
                .finish(),
        }
    }
}

/// It's cheap to clone this struct because it's cheap to clone the bytes
#[derive(Clone)]
pub(crate) struct ChangesBlockBytes {
    bytes: Bytes,
    header: OnceCell<Arc<ChangesBlockHeader>>,
}

impl ChangesBlockBytes {
    fn new(bytes: Bytes) -> Self {
        Self {
            header: OnceCell::new(),
            bytes,
        }
    }

    fn ensure_header(&self) -> LoroResult<()> {
        self.header
            .get_or_init(|| Arc::new(decode_header(&self.bytes).unwrap()));
        Ok(())
    }

    fn parse(&self, a: &SharedArena) -> LoroResult<Vec<Change>> {
        self.ensure_header()?;
        decode_block(&self.bytes, a, self.header.get().map(|h| h.as_ref()))
    }

    fn serialize(changes: &[Change], a: &SharedArena) -> Self {
        let bytes = encode_block(changes, a);
        // TODO: Perf we can calculate header directly without parsing the bytes
        let bytes = ChangesBlockBytes::new(Bytes::from(bytes));
        bytes.ensure_header().unwrap();
        bytes
    }

    fn peer(&mut self) -> PeerID {
        self.ensure_header().unwrap();
        self.header.get().as_ref().unwrap().peer
    }

    fn counter_range(&mut self) -> (Counter, Counter) {
        if let Some(header) = self.header.get() {
            (header.counter, *header.counters.last().unwrap())
        } else {
            decode_block_range(&self.bytes).unwrap().0
        }
    }

    fn lamport_range(&mut self) -> (Lamport, Lamport) {
        if let Some(header) = self.header.get() {
            (header.lamports[0], *header.lamports.last().unwrap())
        } else {
            decode_block_range(&self.bytes).unwrap().1
        }
    }

    /// Length of the changes
    fn len_changes(&self) -> usize {
        self.ensure_header().unwrap();
        self.header.get().unwrap().n_changes
    }

    fn find_deps_for(&mut self, id: ID) -> Frontiers {
        unimplemented!()
    }
}
