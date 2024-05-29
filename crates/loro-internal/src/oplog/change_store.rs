use bytes::Bytes;
use loro_common::{Counter, HasLamportSpan, Lamport, LoroError, LoroResult, PeerID, ID};
use rle::{HasLength, Mergable, RlePush};
use std::{cmp::Ordering, collections::BTreeMap, io::Read, sync::Arc};
mod block_encode;
mod delta_rle_encode;
use crate::{
    arena::SharedArena, change::Change, estimated_size::EstimatedSize, version::Frontiers,
};

use self::block_encode::{decode_block, decode_header, encode_block, ChangesBlockHeader};

const MAX_BLOCK_SIZE: usize = 1024 * 4;

#[derive(Debug, Clone)]
pub struct ChangeStore {
    arena: SharedArena,
    kv: BTreeMap<ID, Arc<ChangesBlock>>,
}

impl ChangeStore {
    pub fn new(a: &SharedArena) -> Self {
        Self {
            arena: a.clone(),
            kv: BTreeMap::new(),
        }
    }

    pub fn insert_change(&mut self, mut change: Change) {
        let id = change.id;
        if let Some((_id, block)) = self.kv.range_mut(..id).next_back() {
            match block.push_change(change) {
                Ok(_) => {
                    return;
                }
                Err(c) => change = c,
            }
        }

        self.kv
            .insert(id, Arc::new(ChangesBlock::new(change, &self.arena)));
    }

    pub fn insert_block(&mut self, block: ChangesBlock) {
        unimplemented!()
    }

    pub fn block_num(&self) -> usize {
        self.kv.len()
    }

    pub(crate) fn iter_bytes(&mut self) -> impl Iterator<Item = (ID, ChangesBlockBytes)> + '_ {
        self.kv
            .iter_mut()
            .map(|(id, block)| (*id, block.bytes(&self.arena)))
    }

    pub(crate) fn encode_all(&mut self) -> Vec<u8> {
        println!("block num {}", self.kv.len());
        let mut bytes = Vec::new();
        for (_, block) in self.iter_bytes() {
            println!("block size {}", block.bytes.len());
            leb128::write::unsigned(&mut bytes, block.bytes.len() as u64).unwrap();
            bytes.extend(&block.bytes);
        }

        bytes
    }

    pub(crate) fn decode_all(&mut self, blocks: &[u8]) -> Result<(), LoroError> {
        assert!(self.kv.is_empty());
        let mut reader = blocks;
        while !reader.is_empty() {
            let size = leb128::read::unsigned(&mut reader).unwrap();
            let block_bytes = &reader[0..size as usize];
            let block = ChangesBlock::from_bytes(Bytes::copy_from_slice(block_bytes), &self.arena)?;
            self.kv.insert(block.id(), Arc::new(block));
            reader = &reader[size as usize..];
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ChangesBlock {
    arena: SharedArena,
    peer: PeerID,
    counter_range: (Counter, Counter),
    lamport_range: (Lamport, Lamport),
    /// Estimated size of the block in bytes
    estimated_size: usize,
    content: ChangesBlockContent,
}

impl ChangesBlock {
    pub fn from_bytes(bytes: Bytes, arena: &SharedArena) -> LoroResult<Self> {
        let len = bytes.len();
        let mut bytes = ChangesBlockBytes::new(bytes);
        let peer = bytes.peer();
        let counter_range = bytes.counter_range();
        let lamport_range = bytes.lamport_range();
        let content = ChangesBlockContent::Bytes(bytes);
        Ok(Self {
            arena: arena.clone(),
            peer,
            estimated_size: len,
            counter_range,
            lamport_range,
            content,
        })
    }

    pub fn new(change: Change, a: &SharedArena) -> Self {
        let atom_len = change.atom_len();
        let counter_range = (change.id.counter, change.id.counter + atom_len as Counter);
        let lamport_range = (change.lamport, change.lamport + atom_len as Lamport);
        let estimated_size = change.estimate_storage_size();
        let peer = change.id.peer;
        let content = ChangesBlockContent::Changes(Arc::new(vec![change]));
        Self {
            arena: a.clone(),
            peer,
            counter_range,
            lamport_range,
            estimated_size,
            content,
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

    pub fn push_change(self: &mut Arc<Self>, change: Change) -> Result<(), Change> {
        if self.counter_range.1 != change.id.counter {
            return Err(change);
        }

        let atom_len = change.atom_len();
        let next_lamport = change.lamport + atom_len as Lamport;
        let next_counter = change.id.counter + atom_len as Counter;

        let is_full = self.is_full();
        let this = Arc::make_mut(self);
        let changes = this.content.changes_mut().unwrap();
        let merge_interval = 10000; // TODO: FIXME: Use configure
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
                let c = Arc::clone(&changes);
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Both(c, bytes.clone());
                bytes
            }
        }
    }

    fn id(&self) -> ID {
        ID::new(self.peer, self.counter_range.0)
    }
}

#[derive(Clone)]
enum ChangesBlockContent {
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
    header: Option<Arc<ChangesBlockHeader>>,
}

impl ChangesBlockBytes {
    fn new(bytes: Bytes) -> Self {
        Self {
            header: None,
            bytes,
        }
    }

    fn ensure_header(&mut self) -> LoroResult<()> {
        if self.header.is_none() {
            self.header = Some(Arc::new(decode_header(&self.bytes)?));
        }
        Ok(())
    }

    fn parse(&mut self, a: &SharedArena) -> LoroResult<Vec<Change>> {
        self.ensure_header()?;
        decode_block(&self.bytes, a, self.header.as_ref().map(|h| h.as_ref()))
    }

    fn serialize(changes: &[Change], a: &SharedArena) -> Self {
        let bytes = encode_block(changes, a);
        // TODO: Perf we can calculate header directly without parsing the bytes
        let mut bytes = ChangesBlockBytes::new(Bytes::from(bytes));
        bytes.ensure_header().unwrap();
        bytes
    }

    fn peer(&mut self) -> PeerID {
        self.ensure_header().unwrap();
        self.header.as_ref().unwrap().peer
    }

    fn counter_range(&mut self) -> (Counter, Counter) {
        self.ensure_header().unwrap();
        (
            self.header.as_ref().unwrap().counter,
            *self.header.as_ref().unwrap().counters.last().unwrap(),
        )
    }

    fn lamport_range(&mut self) -> (Lamport, Lamport) {
        self.ensure_header().unwrap();
        (
            self.header.as_ref().unwrap().lamports[0],
            *self.header.as_ref().unwrap().lamports.last().unwrap(),
        )
    }

    /// Length of the changes
    fn len_changes(&mut self) -> usize {
        self.ensure_header().unwrap();
        self.header.as_ref().unwrap().n_changes
    }

    fn find_deps_for(&mut self, id: ID) -> Frontiers {
        unimplemented!()
    }
}
