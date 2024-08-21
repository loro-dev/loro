use self::block_encode::{decode_block, decode_header, encode_block, ChangesBlockHeader};
use super::{loro_dag::AppDagNodeInner, AppDagNode};
use crate::{
    arena::SharedArena,
    change::{Change, Timestamp},
    estimated_size::EstimatedSize,
    kv_store::KvStore,
    op::Op,
    parent::register_container_and_parent_link,
    version::{Frontiers, ImVersionVector},
    VersionVector,
};
use block_encode::decode_block_range;
use bytes::Bytes;
use itertools::Itertools;
use loro_common::{
    Counter, HasCounterSpan, HasId, HasIdSpan, HasLamportSpan, IdLp, IdSpan, Lamport, LoroError,
    LoroResult, PeerID, ID,
};
use once_cell::sync::OnceCell;
use rle::{HasLength, Mergable, RlePush, RleVec, Sliceable};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, VecDeque},
    ops::{Bound, Deref},
    sync::{atomic::AtomicI64, Arc, Mutex},
};
use tracing::{info_span, trace, warn};

mod block_encode;
mod delta_rle_encode;
pub(super) mod iter;

#[cfg(not(test))]
const MAX_BLOCK_SIZE: usize = 1024 * 4;
#[cfg(test)]
const MAX_BLOCK_SIZE: usize = 128;

/// # Invariance
///
/// - We don't allow holes in a block or between two blocks with the same peer id.
///   The [Change] should be continuous for each peer.
/// - However, the first block of a peer can have counter > 0 so that we can trim the history.
#[derive(Debug, Clone)]
pub struct ChangeStore {
    inner: Arc<Mutex<ChangeStoreInner>>,
    arena: SharedArena,
    /// A change may be in external_kv or in the mem_parsed_kv.
    /// mem_parsed_kv is more up-to-date.
    ///
    /// We cannot directly write into the external_kv except from the initial load
    external_kv: Arc<Mutex<dyn KvStore>>,
    /// The version vector of the external kv store.
    external_vv: Arc<Mutex<VersionVector>>,
    merge_interval: Arc<AtomicI64>,
}

#[derive(Debug, Clone)]
struct ChangeStoreInner {
    /// The start version vector of the first block for each peer.
    /// It allows us to trim the history
    start_vv: ImVersionVector,
    /// The last version of the trimmed history.
    start_frontiers: Frontiers,
    /// It's more like a parsed cache for binary_kv.
    mem_parsed_kv: BTreeMap<ID, Arc<ChangesBlock>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChangesBlock {
    peer: PeerID,
    counter_range: (Counter, Counter),
    lamport_range: (Lamport, Lamport),
    /// Estimated size of the block in bytes
    estimated_size: usize,
    flushed: bool,
    content: ChangesBlockContent,
}

#[derive(Clone)]
pub(crate) enum ChangesBlockContent {
    Changes(Arc<Vec<Change>>),
    Bytes(ChangesBlockBytes),
    Both(Arc<Vec<Change>>, ChangesBlockBytes),
}

/// It's cheap to clone this struct because it's cheap to clone the bytes
#[derive(Clone)]
pub(crate) struct ChangesBlockBytes {
    bytes: Bytes,
    header: OnceCell<Arc<ChangesBlockHeader>>,
}

pub const VV_KEY: &[u8] = b"vv";
pub const FRONTIERS_KEY: &[u8] = b"fr";

impl ChangeStore {
    pub fn new_mem(a: &SharedArena, merge_interval: Arc<AtomicI64>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChangeStoreInner {
                start_vv: ImVersionVector::new(),
                start_frontiers: Frontiers::default(),
                mem_parsed_kv: BTreeMap::new(),
            })),
            arena: a.clone(),
            external_vv: Arc::new(Mutex::new(VersionVector::new())),
            external_kv: Arc::new(Mutex::new(BTreeMap::new())),
            merge_interval,
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        Self::new_mem(&SharedArena::new(), Arc::new(AtomicI64::new(0)))
    }

    pub(super) fn encode_all(&self, vv: &VersionVector, frontiers: &Frontiers) -> Bytes {
        self.flush_and_compact(vv, frontiers);
        self.external_kv.lock().unwrap().export_all()
    }

    pub(crate) fn decode_snapshot_for_updates(
        bytes: Bytes,
        arena: &SharedArena,
        self_vv: &VersionVector,
    ) -> Result<Vec<Change>, LoroError> {
        let change_store = ChangeStore::new_mem(arena, Arc::new(AtomicI64::new(0)));
        let _ = change_store.import_all(bytes)?;
        let mut changes = Vec::new();
        change_store.visit_all_changes(&mut |c| {
            let cnt_threshold = self_vv.get(&c.id.peer).copied().unwrap_or(0);
            if c.id.counter >= cnt_threshold {
                changes.push(c.clone());
                return;
            }

            let change_end = c.ctr_end();
            if change_end > cnt_threshold {
                changes.push(c.slice((cnt_threshold - c.id.counter) as usize, c.atom_len()));
            }
        });

        Ok(changes)
    }
    pub fn get_dag_nodes_that_contains(&self, id: ID) -> Option<Vec<AppDagNode>> {
        let block = self.get_block_that_contains(id)?;
        Some(block.content.iter_dag_nodes())
    }

    pub fn get_last_dag_nodes_for_peer(&self, peer: PeerID) -> Option<Vec<AppDagNode>> {
        let block = self.get_the_last_block_of_peer(peer)?;
        Some(block.content.iter_dag_nodes())
    }

    pub fn visit_all_changes(&self, f: &mut dyn FnMut(&Change)) {
        self.ensure_block_loaded_in_range(Bound::Unbounded, Bound::Unbounded);
        let mut inner = self.inner.lock().unwrap();
        for (_, block) in inner.mem_parsed_kv.iter_mut() {
            block
                .ensure_changes(&self.arena)
                .expect("Parse block error");
            for c in block.content.try_changes().unwrap() {
                f(c);
            }
        }
    }

    pub fn iter_changes(&self, id_span: IdSpan) -> impl Iterator<Item = BlockChangeRef> + '_ {
        self.ensure_block_loaded_in_range(
            Bound::Included(id_span.id_start()),
            Bound::Excluded(id_span.id_end()),
        );
        let mut inner = self.inner.lock().unwrap();
        let start_counter = inner
            .mem_parsed_kv
            .range(..=id_span.id_start())
            .next_back()
            .map(|(id, _)| id.counter)
            .unwrap_or(0);
        let iter = inner
            .mem_parsed_kv
            .range_mut(
                ID::new(id_span.peer, start_counter)..ID::new(id_span.peer, id_span.counter.end),
            )
            .filter_map(|(_id, block)| {
                if block.counter_range.1 < id_span.counter.start {
                    return None;
                }

                block
                    .ensure_changes(&self.arena)
                    .expect("Parse block error");
                let changes = block.content.try_changes().unwrap();
                let start;
                let end;
                if id_span.counter.start <= block.counter_range.0
                    && id_span.counter.end >= block.counter_range.1
                {
                    start = 0;
                    end = changes.len();
                } else {
                    start = block
                        .get_change_index_by_counter(id_span.counter.start)
                        .unwrap_or_else(|x| x);
                    end = block
                        .get_change_index_by_counter(id_span.counter.end)
                        .map_or_else(|e| e, |i| i + 1)
                }
                if start == end {
                    return None;
                }

                Some((block.clone(), start, end))
            })
            // TODO: PERF avoid alloc
            .collect_vec();

        #[cfg(debug_assertions)]
        {
            if !iter.is_empty() {
                assert_eq!(iter[0].0.peer, id_span.peer);
                {
                    // Test start
                    let (block, start, _end) = iter.first().unwrap();
                    let changes = block.content.try_changes().unwrap();
                    assert!(changes[*start].id.counter <= id_span.counter.start);
                }
                {
                    // Test end
                    let (block, _start, end) = iter.last().unwrap();
                    let changes = block.content.try_changes().unwrap();
                    assert!(changes[*end - 1].ctr_end() >= id_span.counter.end);
                }
            }
        }

        iter.into_iter().flat_map(move |(block, start, end)| {
            (start..end).map(move |i| BlockChangeRef {
                change_index: i,
                block: block.clone(),
            })
        })
    }

    pub(crate) fn get_blocks_in_range(&self, id_span: IdSpan) -> VecDeque<Arc<ChangesBlock>> {
        let mut inner = self.inner.lock().unwrap();
        let start_counter = inner
            .mem_parsed_kv
            .range(..=id_span.id_start())
            .next_back()
            .map(|(id, _)| id.counter)
            .unwrap_or(0);
        let vec = inner
            .mem_parsed_kv
            .range_mut(
                ID::new(id_span.peer, start_counter)..ID::new(id_span.peer, id_span.counter.end),
            )
            .filter_map(|(_id, block)| {
                if block.counter_range.1 < id_span.counter.start {
                    return None;
                }

                block
                    .ensure_changes(&self.arena)
                    .expect("Parse block error");
                Some(block.clone())
            })
            // TODO: PERF avoid alloc
            .collect();
        vec
    }

    pub(crate) fn get_block_that_contains(&self, id: ID) -> Option<Arc<ChangesBlock>> {
        self.ensure_block_loaded_in_range(Bound::Included(id), Bound::Included(id));
        let inner = self.inner.lock().unwrap();
        let block = inner
            .mem_parsed_kv
            .range(..=id)
            .next_back()
            .filter(|(_, block)| {
                block.peer == id.peer
                    && block.counter_range.0 <= id.counter
                    && id.counter < block.counter_range.1
            })
            .map(|(_, block)| block.clone());

        block
    }

    pub(crate) fn get_the_last_block_of_peer(&self, peer: PeerID) -> Option<Arc<ChangesBlock>> {
        let end_id = ID::new(peer, Counter::MAX);
        self.ensure_id_lte(end_id);
        let inner = self.inner.lock().unwrap();
        let block = inner
            .mem_parsed_kv
            .range(..=end_id)
            .next_back()
            .filter(|(_, block)| block.peer == peer)
            .map(|(_, block)| block.clone());

        block
    }

    pub fn change_num(&self) -> usize {
        let mut inner = self.inner.lock().unwrap();
        inner
            .mem_parsed_kv
            .iter_mut()
            .map(|(_, block)| block.change_num())
            .sum()
    }

    pub fn fork(&self, arena: SharedArena, merge_interval: Arc<AtomicI64>) -> Self {
        let inner = self.inner.lock().unwrap();
        Self {
            inner: Arc::new(Mutex::new(ChangeStoreInner {
                start_vv: inner.start_vv.clone(),
                start_frontiers: inner.start_frontiers.clone(),
                mem_parsed_kv: BTreeMap::new(),
            })),
            arena,
            external_vv: self.external_vv.clone(),
            external_kv: self.external_kv.lock().unwrap().clone_store(),
            merge_interval,
        }
    }

    pub fn kv_size(&self) -> usize {
        self.external_kv
            .lock()
            .unwrap()
            .scan(Bound::Unbounded, Bound::Unbounded)
            .map(|(k, v)| k.len() + v.len())
            .sum()
    }
}

mod mut_external_kv {
    //! Only this module contains the code that mutate the external kv store
    //! All other modules should only read from the external kv store
    use super::*;

    impl ChangeStore {
        #[tracing::instrument(skip_all, level = "debug", name = "change_store import_all")]
        pub(crate) fn import_all(&self, bytes: Bytes) -> Result<BatchDecodeInfo, LoroError> {
            let mut kv_store = self.external_kv.lock().unwrap();
            assert!(
                // 2 because there are vv and frontiers
                kv_store.len() <= 2,
                "kv store should be empty when using decode_all"
            );
            kv_store
                .import_all(bytes)
                .map_err(|e| LoroError::DecodeError(e.into_boxed_str()))?;
            let vv_bytes = kv_store.get(b"vv").unwrap_or_default();
            let vv = VersionVector::decode(&vv_bytes).unwrap();

            #[cfg(test)]
            {
                // This is for tests
                drop(kv_store);
                for (peer, cnt) in vv.iter() {
                    self.get_change(ID::new(*peer, *cnt - 1)).unwrap();
                }
                kv_store = self.external_kv.lock().unwrap();
            }

            *self.external_vv.lock().unwrap() = vv.clone();
            let frontiers_bytes = kv_store.get(b"fr").unwrap_or_default();
            let frontiers = Frontiers::decode(&frontiers_bytes).unwrap();
            let mut max_lamport = None;
            let mut max_timestamp = 0;
            drop(kv_store);
            for id in frontiers.iter() {
                let c = self.get_change(*id).unwrap();
                debug_assert_ne!(c.atom_len(), 0);
                let l = c.lamport_last();
                if let Some(x) = max_lamport {
                    if l > x {
                        max_lamport = Some(l);
                    }
                } else {
                    max_lamport = Some(l);
                }

                let t = c.timestamp;
                if t > max_timestamp {
                    max_timestamp = t;
                }
            }

            Ok(BatchDecodeInfo {
                vv,
                frontiers,
                next_lamport: match max_lamport {
                    Some(l) => l + 1,
                    None => 0,
                },
                max_timestamp,
            })

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

        /// Flush the cached change to kv_store
        pub(crate) fn flush_and_compact(&self, vv: &VersionVector, frontiers: &Frontiers) {
            let mut inner = self.inner.lock().unwrap();
            let mut store = self.external_kv.lock().unwrap();
            let mut external_vv = self.external_vv.lock().unwrap();
            for (id, block) in inner.mem_parsed_kv.iter_mut() {
                if !block.flushed {
                    let id_bytes = id.to_bytes();
                    let counter_start = external_vv.get(&id.peer).copied().unwrap_or(0);
                    assert!(
                        counter_start >= block.counter_range.0
                            && counter_start < block.counter_range.1
                    );
                    if counter_start > block.counter_range.0 {
                        assert!(store.get(&id_bytes).is_some());
                    }
                    external_vv.insert(id.peer, block.counter_range.1);
                    let bytes = block.to_bytes(&self.arena);
                    store.set(&id_bytes, bytes.bytes);
                    Arc::make_mut(block).flushed = true;
                }
            }

            assert_eq!(&*external_vv, vv);
            let vv_bytes = vv.encode();
            let frontiers_bytes = frontiers.encode();
            store.set(VV_KEY, vv_bytes.into());
            store.set(FRONTIERS_KEY, frontiers_bytes.into());
        }
    }
}

mod mut_inner_kv {
    //! Only this module contains the code that mutate the internal kv store
    //! All other modules should only read from the internal kv store

    use super::*;
    impl ChangeStore {
        /// This method is the **only place** that push a new change into the change store
        ///
        /// The new change either merges with the previous block or is put into a new block.
        /// This method only updates the internal kv store.
        pub fn insert_change(&self, mut change: Change, split_when_exceeds: bool) {
            #[cfg(debug_assertions)]
            {
                let vv = self.external_vv.lock().unwrap();
                assert!(vv.get(&change.id.peer).copied().unwrap_or(0) <= change.id.counter);
            }

            let s = info_span!("change_store insert_change", id = ?change.id);
            let _e = s.enter();
            let estimated_size = change.estimate_storage_size();
            if estimated_size > MAX_BLOCK_SIZE && split_when_exceeds {
                self.split_change_then_insert(change);
                return;
            }

            let id = change.id;
            let mut inner = self.inner.lock().unwrap();

            // try to merge with previous block
            if let Some((_id, block)) = inner.mem_parsed_kv.range_mut(..id).next_back() {
                if block.peer == change.id.peer {
                    if block.counter_range.1 != change.id.counter {
                        panic!("counter should be continuous")
                    }

                    match block.push_change(
                        change,
                        estimated_size,
                        self.merge_interval
                            .load(std::sync::atomic::Ordering::Acquire),
                        &self.arena,
                    ) {
                        Ok(_) => {
                            drop(inner);
                            debug_assert!(self.get_change(id).is_some());
                            return;
                        }
                        Err(c) => change = c,
                    }
                }
            }

            inner
                .mem_parsed_kv
                .insert(id, Arc::new(ChangesBlock::new(change, &self.arena)));
            drop(inner);
            debug_assert!(self.get_change(id).is_some());
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
        pub fn get_change_by_lamport_lte(&self, idlp: IdLp) -> Option<BlockChangeRef> {
            // This method is complicated because we impl binary search on top of the range api
            // It can be simplified
            let mut inner = self.inner.lock().unwrap();
            let mut iter = inner
                .mem_parsed_kv
                .range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, i32::MAX));

            // This won't change, we only adjust upper_bound
            let mut lower_bound = 0;
            let mut upper_bound = i32::MAX;
            let mut is_binary_searching = false;
            loop {
                match iter.next_back() {
                    Some((&id, block)) => {
                        if block.lamport_range.0 <= idlp.lamport
                            && (!is_binary_searching || idlp.lamport < block.lamport_range.1)
                        {
                            if !is_binary_searching
                                && upper_bound != i32::MAX
                                && upper_bound != block.counter_range.1
                            {
                                warn!(
                                    "There is a hole between the last block and the current block"
                                );
                                // There is hole between the last block and the current block
                                // We need to load it from the kv store
                                break;
                            }

                            // Found the block
                            block
                                .ensure_changes(&self.arena)
                                .expect("Parse block error");
                            let index = block.get_change_index_by_lamport_lte(idlp.lamport)?;
                            return Some(BlockChangeRef {
                                change_index: index,
                                block: block.clone(),
                            });
                        }

                        if is_binary_searching {
                            let mid_bound = (lower_bound + upper_bound) / 2;
                            if block.lamport_range.1 <= idlp.lamport {
                                // Target is larger than the current block (pointed by mid_bound)
                                lower_bound = mid_bound;
                            } else {
                                debug_assert!(
                                    idlp.lamport < block.lamport_range.0,
                                    "{} {:?}",
                                    idlp,
                                    &block.lamport_range
                                );
                                // Target is smaller than the current block (pointed by mid_bound)
                                upper_bound = mid_bound;
                            }

                            let mid_bound = (lower_bound + upper_bound) / 2;
                            iter = inner
                                .mem_parsed_kv
                                .range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, mid_bound));
                        } else {
                            // Test whether we need to switch to binary search by measuring the gap
                            if block.lamport_range.0 - idlp.lamport > MAX_BLOCK_SIZE as Lamport * 8
                            {
                                // Use binary search to find the block
                                upper_bound = id.counter;
                                let mid_bound = (lower_bound + upper_bound) / 2;
                                iter = inner.mem_parsed_kv.range_mut(
                                    ID::new(idlp.peer, 0)..ID::new(idlp.peer, mid_bound),
                                );
                                trace!("Use binary search");
                                is_binary_searching = true;
                            }

                            upper_bound = id.counter;
                        }
                    }
                    None => {
                        if !is_binary_searching {
                            trace!("No parsed block found, break");
                            break;
                        }

                        let mid_bound = (lower_bound + upper_bound) / 2;
                        lower_bound = mid_bound;
                        if upper_bound - lower_bound <= MAX_BLOCK_SIZE as i32 {
                            // If they are too close, we can just scan the range
                            iter = inner.mem_parsed_kv.range_mut(
                                ID::new(idlp.peer, lower_bound)..ID::new(idlp.peer, upper_bound),
                            );
                            is_binary_searching = false;
                        } else {
                            let mid_bound = (lower_bound + upper_bound) / 2;
                            iter = inner
                                .mem_parsed_kv
                                .range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, mid_bound));
                        }
                    }
                }
            }

            let counter_end = upper_bound;
            let scan_end = ID::new(idlp.peer, counter_end).to_bytes();
            trace!(
                "Scanning from {:?} to {:?}",
                &ID::new(idlp.peer, 0),
                &counter_end
            );

            let (id, bytes) = 'block_scan: {
                let kv_store = &self.external_kv.lock().unwrap();
                let iter = kv_store
                    .scan(
                        Bound::Included(&ID::new(idlp.peer, 0).to_bytes()),
                        Bound::Excluded(&scan_end),
                    )
                    .rev();

                for (id, bytes) in iter {
                    let mut block = ChangesBlockBytes::new(bytes.clone());
                    trace!(
                        "Scanning block counter_range={:?} lamport_range={:?} id={:?}",
                        &block.counter_range(),
                        &block.lamport_range(),
                        &ID::from_bytes(&id)
                    );
                    let (lamport_start, _lamport_end) = block.lamport_range();
                    if lamport_start <= idlp.lamport {
                        break 'block_scan (id, bytes);
                    }
                }

                return None;
            };

            let block_id = ID::from_bytes(&id);
            let mut block = Arc::new(ChangesBlock::from_bytes(bytes).unwrap());
            block
                .ensure_changes(&self.arena)
                .expect("Parse block error");
            inner.mem_parsed_kv.insert(block_id, block.clone());
            let index = block.get_change_index_by_lamport_lte(idlp.lamport)?;
            Some(BlockChangeRef {
                change_index: index,
                block,
            })
        }

        fn split_change_then_insert(&self, change: Change) {
            let original_len = change.atom_len();
            let mut new_change = Change {
                ops: RleVec::new(),
                deps: change.deps,
                id: change.id,
                lamport: change.lamport,
                timestamp: change.timestamp,
            };

            let mut total_len = 0;
            let mut estimated_size = new_change.estimate_storage_size();
            'outer: for mut op in change.ops.into_iter() {
                if op.estimate_storage_size() >= MAX_BLOCK_SIZE - estimated_size {
                    new_change = self._insert_splitted_change(
                        new_change,
                        &mut total_len,
                        &mut estimated_size,
                    );
                }

                while let Some(end) =
                    op.check_whether_slice_content_to_fit_in_size(MAX_BLOCK_SIZE - estimated_size)
                {
                    // The new op can take the rest of the room
                    let new = op.slice(0, end);
                    new_change.ops.push(new);
                    new_change = self._insert_splitted_change(
                        new_change,
                        &mut total_len,
                        &mut estimated_size,
                    );

                    if end < op.atom_len() {
                        op = op.slice(end, op.atom_len());
                    } else {
                        continue 'outer;
                    }
                }

                estimated_size += op.estimate_storage_size();
                if estimated_size > MAX_BLOCK_SIZE && !new_change.ops.is_empty() {
                    new_change = self._insert_splitted_change(
                        new_change,
                        &mut total_len,
                        &mut estimated_size,
                    );
                    new_change.ops.push(op);
                } else {
                    new_change.ops.push(op);
                }
            }

            if !new_change.ops.is_empty() {
                total_len += new_change.atom_len();
                self.insert_change(new_change, false);
            }

            debug_assert_eq!(total_len, original_len);
        }

        fn _insert_splitted_change(
            &self,
            new_change: Change,
            total_len: &mut usize,
            estimated_size: &mut usize,
        ) -> Change {
            if new_change.atom_len() == 0 {
                return new_change;
            }

            let ctr_end = new_change.id.counter + new_change.atom_len() as Counter;
            let next_lamport = new_change.lamport + new_change.atom_len() as Lamport;
            *total_len += new_change.atom_len();
            let ans = Change {
                ops: RleVec::new(),
                deps: ID::new(new_change.id.peer, ctr_end - 1).into(),
                id: ID::new(new_change.id.peer, ctr_end),
                lamport: next_lamport,
                timestamp: new_change.timestamp,
            };

            self.insert_change(new_change, false);
            *estimated_size = ans.estimate_storage_size();
            ans
        }

        fn get_parsed_block(&self, id: ID) -> Option<Arc<ChangesBlock>> {
            let mut inner = self.inner.lock().unwrap();
            // trace!("inner: {:#?}", &inner);
            if let Some((_id, block)) = inner.mem_parsed_kv.range_mut(..=id).next_back() {
                if block.peer == id.peer && block.counter_range.1 > id.counter {
                    block
                        .ensure_changes(&self.arena)
                        .expect("Parse block error");
                    return Some(block.clone());
                }
            }

            let store = self.external_kv.lock().unwrap();
            let mut iter = store
                .scan(Bound::Unbounded, Bound::Included(&id.to_bytes()))
                .filter(|(id, _)| id.len() == 12);
            let (b_id, b_bytes) = iter.next_back()?;
            let block_id: ID = ID::from_bytes(&b_id[..]);
            let block = ChangesBlock::from_bytes(b_bytes).unwrap();
            trace!(
                "block_id={:?} id={:?} counter_range={:?}",
                block_id,
                id,
                block.counter_range
            );
            if block_id.peer == id.peer
                && block_id.counter <= id.counter
                && block.counter_range.1 > id.counter
            {
                let mut arc_block = Arc::new(block);
                arc_block
                    .ensure_changes(&self.arena)
                    .expect("Parse block error");
                inner.mem_parsed_kv.insert(block_id, arc_block.clone());
                return Some(arc_block);
            }

            None
        }

        /// Load all the blocks that have overlapped with the given ID range into `inner_mem_parsed_kv`
        ///
        /// This is fast because we don't actually parse the content.
        // TODO: PERF: This method feels slow.
        pub(super) fn ensure_block_loaded_in_range(&self, start: Bound<ID>, end: Bound<ID>) {
            let mut whether_need_scan_backward = match start {
                Bound::Included(id) => Some(id),
                Bound::Excluded(id) => Some(id.inc(1)),
                Bound::Unbounded => None,
            };

            {
                let start = start.map(|id| id.to_bytes());
                let end = end.map(|id| id.to_bytes());
                let kv = self.external_kv.lock().unwrap();
                let mut inner = self.inner.lock().unwrap();
                for (id, bytes) in kv
                    .scan(
                        start.as_ref().map(|x| x.as_slice()),
                        end.as_ref().map(|x| x.as_slice()),
                    )
                    .filter(|(id, _)| id.len() == 12)
                {
                    let id = ID::from_bytes(&id);
                    if let Some(expected_start_id) = whether_need_scan_backward {
                        if id == expected_start_id {
                            whether_need_scan_backward = None;
                        }
                    }

                    if inner.mem_parsed_kv.contains_key(&id) {
                        continue;
                    }

                    let block = ChangesBlock::from_bytes(bytes.clone()).unwrap();
                    inner.mem_parsed_kv.insert(id, Arc::new(block));
                }
            }

            if let Some(start_id) = whether_need_scan_backward {
                self.ensure_id_lte(start_id);
            }
        }

        pub(super) fn ensure_id_lte(&self, id: ID) {
            let kv = self.external_kv.lock().unwrap();
            let mut inner = self.inner.lock().unwrap();
            let Some((next_back_id, next_back_bytes)) = kv
                .scan(Bound::Unbounded, Bound::Included(&id.to_bytes()))
                .filter(|(id, _)| id.len() == 12)
                .next_back()
            else {
                return;
            };

            let next_back_id = ID::from_bytes(&next_back_id);
            if next_back_id.peer == id.peer {
                if inner.mem_parsed_kv.contains_key(&next_back_id) {
                    return;
                }

                let block = ChangesBlock::from_bytes(next_back_bytes).unwrap();
                inner.mem_parsed_kv.insert(next_back_id, Arc::new(block));
            }
        }
    }
}

#[must_use]
#[derive(Clone, Debug)]
pub(crate) struct BatchDecodeInfo {
    pub vv: VersionVector,
    pub frontiers: Frontiers,
    pub next_lamport: Lamport,
    pub max_timestamp: Timestamp,
}

#[derive(Clone, Debug)]
pub struct BlockChangeRef {
    block: Arc<ChangesBlock>,
    change_index: usize,
}

impl Deref for BlockChangeRef {
    type Target = Change;
    fn deref(&self) -> &Change {
        &self.block.content.try_changes().unwrap()[self.change_index]
    }
}

impl BlockChangeRef {
    pub(crate) fn get_op_with_counter(&self, counter: Counter) -> Option<BlockOpRef> {
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
pub(crate) struct BlockOpRef {
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

impl BlockOpRef {
    pub fn lamport(&self) -> Lamport {
        let change = &self.block.content.try_changes().unwrap()[self.change_index];
        let op = &change.ops[self.op_index];
        (op.counter - change.id.counter) as Lamport + change.lamport
    }
}

impl ChangesBlock {
    fn from_bytes(bytes: Bytes) -> LoroResult<Self> {
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
            flushed: true,
            content,
        })
    }

    pub(crate) fn content(&self) -> &ChangesBlockContent {
        &self.content
    }

    fn new(change: Change, _a: &SharedArena) -> Self {
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

    #[allow(unused)]
    fn cmp_id(&self, id: ID) -> Ordering {
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

    #[allow(unused)]
    fn cmp_idlp(&self, idlp: (PeerID, Lamport)) -> Ordering {
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

    #[allow(unused)]
    fn is_full(&self) -> bool {
        self.estimated_size > MAX_BLOCK_SIZE
    }

    fn push_change(
        self: &mut Arc<Self>,
        change: Change,
        new_change_size: usize,
        merge_interval: i64,
        a: &SharedArena,
    ) -> Result<(), Change> {
        if self.counter_range.1 != change.id.counter {
            return Err(change);
        }

        let atom_len = change.atom_len();
        let next_lamport = change.lamport + atom_len as Lamport;
        let next_counter = change.id.counter + atom_len as Counter;

        let is_full = new_change_size + self.estimated_size > MAX_BLOCK_SIZE;
        let this = Arc::make_mut(self);
        let changes = this.content.changes_mut(a).unwrap();
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
                    this.estimated_size += new_change_size;
                    changes.push(change);
                }
            }
        }

        this.flushed = false;
        this.counter_range.1 = next_counter;
        this.lamport_range.1 = next_lamport;
        Ok(())
    }

    fn to_bytes<'a>(self: &'a mut Arc<Self>, a: &SharedArena) -> ChangesBlockBytes {
        match &self.content {
            ChangesBlockContent::Bytes(bytes) => bytes.clone(),
            ChangesBlockContent::Both(_, bytes) => {
                let bytes = bytes.clone();
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Bytes(bytes.clone());
                bytes
            }
            ChangesBlockContent::Changes(changes) => {
                let bytes = ChangesBlockBytes::serialize(changes, a);
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Bytes(bytes.clone());
                bytes
            }
        }
    }

    fn ensure_changes(self: &mut Arc<Self>, a: &SharedArena) -> LoroResult<()> {
        match &self.content {
            ChangesBlockContent::Changes(_) => Ok(()),
            ChangesBlockContent::Both(_, _) => Ok(()),
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(a)?;
                let b = bytes.clone();
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Both(Arc::new(changes), b);
                Ok(())
            }
        }
    }

    fn get_change_index_by_counter(&self, counter: Counter) -> Result<usize, usize> {
        let changes = self.content.try_changes().unwrap();
        changes.binary_search_by(|c| {
            if c.id.counter > counter {
                Ordering::Greater
            } else if (c.id.counter + c.content_len() as Counter) <= counter {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    fn get_change_index_by_lamport_lte(&self, lamport: Lamport) -> Option<usize> {
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
            Err(idx) => {
                if idx == 0 {
                    None
                } else {
                    Some(idx - 1)
                }
            }
        }
    }

    #[allow(unused)]
    fn get_changes(&mut self, a: &SharedArena) -> LoroResult<&Vec<Change>> {
        self.content.changes(a)
    }

    #[allow(unused)]
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

impl ChangesBlockContent {
    // TODO: PERF: We can use Iter to replace Vec
    pub fn iter_dag_nodes(&self) -> Vec<AppDagNode> {
        let mut dag_nodes = Vec::new();
        match self {
            ChangesBlockContent::Changes(c) | ChangesBlockContent::Both(c, _) => {
                for change in c.iter() {
                    let new_node = AppDagNodeInner {
                        peer: change.id.peer,
                        cnt: change.id.counter,
                        lamport: change.lamport,
                        deps: change.deps.clone(),
                        vv: OnceCell::new(),
                        has_succ: false,
                        len: change.atom_len(),
                    }
                    .into();

                    dag_nodes.push_rle_element(new_node);
                }
            }
            ChangesBlockContent::Bytes(b) => {
                b.ensure_header().unwrap();
                let header = b.header.get().unwrap();
                let n = header.n_changes;
                for i in 0..n {
                    let new_node = AppDagNodeInner {
                        peer: header.peer,
                        cnt: header.counters[i],
                        lamport: header.lamports[i],
                        deps: header.deps_groups[i].clone(),
                        vv: OnceCell::new(),
                        has_succ: false,
                        len: (header.counters[i + 1] - header.counters[i]) as usize,
                    }
                    .into();

                    dag_nodes.push_rle_element(new_node);
                }
            }
        }

        dag_nodes
    }

    #[allow(unused)]
    pub fn changes(&mut self, a: &SharedArena) -> LoroResult<&Vec<Change>> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => Ok(changes),
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(a)?;
                *self = ChangesBlockContent::Both(Arc::new(changes), bytes.clone());
                self.changes(a)
            }
        }
    }

    /// Note that this method will invalidate the stored bytes
    fn changes_mut(&mut self, a: &SharedArena) -> LoroResult<&mut Arc<Vec<Change>>> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => {
                *self = ChangesBlockContent::Changes(std::mem::take(changes));
                self.changes_mut(a)
            }
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(a)?;
                *self = ChangesBlockContent::Changes(Arc::new(changes));
                self.changes_mut(a)
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
        let ans: Vec<Change> = decode_block(&self.bytes, a, self.header.get().map(|h| h.as_ref()))?;
        for c in ans.iter() {
            // PERF: This can be made faster (low priority)
            register_container_and_parent_link(a, c)
        }

        Ok(ans)
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

    #[allow(unused)]
    fn find_deps_for(&mut self, id: ID) -> Frontiers {
        unimplemented!()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        oplog::convert_change_to_remote, ListHandler, LoroDoc, MovableListHandler, TextHandler,
        TreeHandler,
    };

    use super::*;

    fn test_encode_decode(doc: LoroDoc) {
        let oplog = doc.oplog().lock().unwrap();
        let bytes = oplog
            .change_store
            .encode_all(oplog.vv(), oplog.dag.frontiers());
        let store = ChangeStore::new_for_test();
        let _ = store.import_all(bytes.clone()).unwrap();
        assert_eq!(store.external_kv.lock().unwrap().export_all(), bytes);
        let mut changes_parsed = Vec::new();
        let a = store.arena.clone();
        store.visit_all_changes(&mut |c| {
            changes_parsed.push(convert_change_to_remote(&a, c));
        });
        let mut changes = Vec::new();
        oplog.change_store.visit_all_changes(&mut |c| {
            changes.push(convert_change_to_remote(&oplog.arena, c));
        });
        assert_eq!(changes_parsed, changes);
    }

    #[test]
    fn test_change_store() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_record_timestamp(true);
        let t = doc.get_text("t");
        t.insert(0, "hello").unwrap();
        doc.commit_then_renew();
        let t = doc.get_list("t");
        t.insert(0, "hello").unwrap();
        test_encode_decode(doc);
    }

    #[test]
    fn test_synced_doc() -> LoroResult<()> {
        let doc_a = LoroDoc::new_auto_commit();
        let doc_b = LoroDoc::new_auto_commit();
        let doc_c = LoroDoc::new_auto_commit();

        {
            // A: Create initial structure
            let map = doc_a.get_map("root");
            map.insert_container("text", TextHandler::new_detached())?;
            map.insert_container("list", ListHandler::new_detached())?;
            map.insert_container("tree", TreeHandler::new_detached())?;
        }

        {
            // Sync initial state to B and C
            let initial_state = doc_a.export_from(&Default::default());
            doc_b.import(&initial_state)?;
            doc_c.import(&initial_state)?;
        }

        {
            // B: Edit text and list
            let map = doc_b.get_map("root");
            let text = map
                .insert_container("text", TextHandler::new_detached())
                .unwrap();
            text.insert(0, "Hello, ")?;

            let list = map
                .insert_container("list", ListHandler::new_detached())
                .unwrap();
            list.push("world")?;
        }

        {
            // C: Edit tree and movable list
            let map = doc_c.get_map("root");
            let tree = map
                .insert_container("tree", TreeHandler::new_detached())
                .unwrap();
            let node_id = tree.create(None)?;
            tree.get_meta(node_id)?.insert("key", "value")?;
            let node_b = tree.create(None)?;
            tree.move_to(node_b, None, 0).unwrap();

            let movable_list = map
                .insert_container("movable", MovableListHandler::new_detached())
                .unwrap();
            movable_list.push("item1".into())?;
            movable_list.push("item2".into())?;
            movable_list.mov(0, 1)?;
        }

        // Sync B's changes to A
        let b_changes = doc_b.export_from(&doc_a.oplog_vv());
        doc_a.import(&b_changes)?;

        // Sync C's changes to A
        let c_changes = doc_c.export_from(&doc_a.oplog_vv());
        doc_a.import(&c_changes)?;

        test_encode_decode(doc_a);
        Ok(())
    }
}
