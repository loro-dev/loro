use std::{borrow::Cow, sync::Arc};

use fxhash::{FxHashMap, FxHashSet};
use loro_common::{
    ContainerID, ContainerType, Counter, HasCounterSpan, HasId, HasIdSpan, HasLamportSpan,
    InternalString, LoroResult, PeerID, ID,
};
use num_traits::{FromPrimitive, ToPrimitive};
use rle::{HasLength, Sliceable};
use serde_columnar::columnar;

use crate::{
    change::Change,
    container::{idx::ContainerIdx, list::list_op::DeleteSpan, richtext::TextStyleInfoFlag},
    encoding::encode_reordered::value::{EncodedTreeMove, ValueKind, ValueWriter},
    op::{Op, SliceRange},
    version::Frontiers,
    OpLog, VersionVector,
};

use self::{
    arena::{decode_arena, encode_arena, ContainerArena, DecodedArenas},
    value::{MarkStart, ValueReader},
};

pub(crate) fn encode(oplog: &OpLog, vv: &VersionVector) -> Vec<u8> {
    let mut peer_id_to_idx: FxHashMap<PeerID, PeerIdx> = FxHashMap::default();
    let mut peers = Vec::with_capacity(oplog.changes().len());
    let mut diff_changes = Vec::new();
    let self_vv = oplog.vv();
    let start_vv = vv.trim(&oplog.vv());
    let diff = self_vv.diff(&start_vv);

    let mut start_counter = Vec::new();

    for span in diff.left.iter() {
        let id = span.id_start();
        let changes = oplog.get_change_at(id).unwrap();
        let peer_id = *span.0;
        let idx = peers.len() as PeerIdx;
        peers.push(peer_id);
        peer_id_to_idx.insert(peer_id, idx);
        start_counter.push(
            changes
                .id
                .counter
                .max(start_vv.get(&peer_id).copied().unwrap_or(0)),
        );
    }

    for (change, _) in oplog.iter_causally(start_vv.clone(), self_vv.clone()) {
        let start_cnt = start_vv.get(&change.id.peer).copied().unwrap_or(0);
        if change.id.counter < start_cnt {
            let offset = start_cnt - change.id.counter;
            diff_changes.push(Cow::Owned(change.slice(offset as usize, change.atom_len())));
        } else {
            diff_changes.push(Cow::Borrowed(change));
        }
    }

    let (mut containers, container_idx2index) = extract_containers_in_order(&diff_changes, oplog);

    let mut cid2index = containers
        .iter()
        .enumerate()
        .map(|(i, cid)| (cid.clone(), i))
        .collect::<FxHashMap<_, _>>();
    let mut register_cid = |cid: &ContainerID| -> usize {
        *cid2index.entry(cid.clone()).or_insert_with(|| {
            let idx = containers.len();
            containers.push(cid.clone());
            idx
        })
    };

    let mut keys: Vec<InternalString> = Vec::new();
    let mut key_to_idx: FxHashMap<InternalString, usize> = FxHashMap::default();

    let mut register_key = |key: &InternalString| -> usize {
        if let Some(ans) = key_to_idx.get(key) {
            return *ans;
        }

        *key_to_idx.entry(key.clone()).or_insert_with(|| {
            let idx = keys.len();
            keys.push(key.clone());
            idx
        })
    };

    let mut register_peer_id = |peer_id: PeerID| -> usize {
        *peer_id_to_idx.entry(peer_id).or_insert_with(|| {
            let idx = peers.len();
            peers.push(peer_id);
            idx
        })
    };

    let mut dep_arena = arena::DepsArena::default();
    let mut changes: Vec<EncodedChange> = Vec::with_capacity(diff_changes.len());
    let mut value_writer = ValueWriter::new();
    let mut ops: Vec<EncodedOp> = Vec::new();
    let arena = &oplog.arena;
    for change in diff_changes {
        let mut dep_on_self = false;
        let mut deps_len = 0;
        for dep in change.deps.iter() {
            if dep.peer == change.id.peer {
                dep_on_self = true;
            } else {
                deps_len += 1;
                dep_arena.push(register_peer_id(dep.peer), dep.counter);
            }
        }

        let peer_idx = register_peer_id(change.id.peer);
        changes.push(EncodedChange {
            dep_on_self,
            deps_len,
            peer_idx,
            counter: change.id.counter,
            lamport: change.lamport,
            len: change.atom_len(),
            timestamp: change.timestamp,
            msg_len: 0,
        });

        for op in change.ops().iter() {
            let container_index = container_idx2index[&op.container] as u32;
            let (prop, value_type) = encode_op(
                op,
                arena,
                &mut value_writer,
                &mut register_key,
                &mut register_cid,
                &mut register_peer_id,
            );

            ops.push(EncodedOp {
                container_index,
                peer_idx: peer_idx as u32,
                counter: op.counter,
                prop,
                value_type: value_type.to_u8().unwrap(),
            })
        }
    }

    let container_arena =
        ContainerArena::from_containers(containers, &mut register_peer_id, &mut register_key);

    let doc = EncodedDoc {
        ops,
        changes,
        raw_values: Cow::Owned(value_writer.finish()),
        arenas: Cow::Owned(encode_arena(peers, container_arena, keys, dep_arena)),
    };

    serde_columnar::to_vec(&doc).unwrap()
}

pub(crate) fn decode(oplog: &mut OpLog, bytes: &[u8]) -> LoroResult<()> {
    let iter = serde_columnar::iter_from_bytes::<EncodedDoc>(bytes)?;
    let DecodedArenas {
        peer_ids,
        containers,
        keys,
        mut deps,
    } = decode_arena(&iter.arenas)?;
    let raw_values = &iter.raw_values;
    let mut value_reader = ValueReader::new(raw_values);
    let mut ops_map: FxHashMap<PeerID, Vec<Op>> = FxHashMap::default();
    let arena = &oplog.arena;
    let containers: Vec<_> = containers
        .containers
        .iter()
        .map(|x| x.to_container_id(&keys.keys, &peer_ids.peer_ids))
        .collect();
    for EncodedOp {
        container_index,
        prop,
        peer_idx,
        value_type,
        counter,
    } in iter.ops
    {
        let cid = &containers[container_index as usize];
        let c_idx = arena.register_container(&cid);
        let kind = ValueKind::from_u8(value_type).unwrap();
        let content = decode_op(
            cid,
            kind,
            &mut value_reader,
            arena,
            prop,
            &keys,
            &peer_ids.peer_ids,
            &containers,
        );

        let peer = peer_ids.peer_ids[peer_idx as usize];
        ops_map.entry(peer).or_default().push(Op {
            counter,
            container: c_idx,
            content,
        });
    }

    for (_, ops) in ops_map.iter_mut() {
        // sort op by counter in the reversed order
        ops.sort_by_key(|x| -x.counter);
    }

    let mut changes = Vec::with_capacity(iter.changes.size_hint().0);
    for EncodedChange {
        peer_idx,
        counter,
        lamport,
        mut len,
        timestamp,
        deps_len,
        dep_on_self,
        msg_len: _,
    } in iter.changes
    {
        let peer = peer_ids.peer_ids[peer_idx];
        let mut change: Change = Change {
            id: ID::new(peer, counter),
            ops: Default::default(),
            deps: Frontiers::with_capacity((deps_len + if dep_on_self { 1 } else { 0 }) as usize),
            lamport,
            timestamp,
            has_dependents: false,
        };

        if dep_on_self {
            change.deps.push(ID::new(peer, counter - 1));
        }

        for _ in 0..deps_len {
            let dep = deps.next().unwrap();
            change
                .deps
                .push(ID::new(peer_ids.peer_ids[dep.peer_idx], dep.counter));
        }

        let ops = ops_map.get_mut(&peer).unwrap();
        while len > 0 {
            let op = ops.pop().unwrap();
            len -= op.atom_len();
            change.ops.push(op);
        }

        changes.push(change);
    }

    let mut pending_changes = Vec::new();
    debug_log::debug_dbg!(&changes);
    let mut latest_ids = Vec::new();
    'outer: for mut change in changes {
        if change.ctr_end() <= oplog.vv().get(&change.id.peer).copied().unwrap_or(0) {
            // skip included changes
            continue;
        }

        latest_ids.push(change.id_last());
        // calc lamport or pending if its deps are not satisfied
        for dep in change.deps.iter() {
            match oplog.dag.get_lamport(dep) {
                Some(lamport) => {
                    change.lamport = change.lamport.max(lamport + 1);
                }
                None => {
                    pending_changes.push(change);
                    continue 'outer;
                }
            }
        }

        let Some(change) = oplog.trim_the_known_part_of_change(change) else {
            continue;
        };
        // update dag and push the change
        let mark = oplog.insert_dag_node_on_new_change(&change);
        oplog.next_lamport = oplog.next_lamport.max(change.lamport_end());
        oplog.latest_timestamp = oplog.latest_timestamp.max(change.timestamp);
        oplog.dag.vv.extend_to_include_end_id(ID {
            peer: change.id.peer,
            counter: change.id.counter + change.atom_len() as Counter,
        });
        oplog.insert_new_change(change, mark);
    }

    let mut vv = oplog.dag.vv.clone();
    oplog.try_apply_pending(latest_ids, &mut vv);
    if !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }

    oplog.import_unknown_lamport_pending_changes(pending_changes)?;
    Ok(())
}

fn encode_op(
    op: &Op,
    arena: &crate::arena::SharedArena,
    value_writer: &mut ValueWriter,
    register_key: &mut impl FnMut(&string_cache::Atom<string_cache::EmptyStaticAtomSet>) -> usize,
    register_cid: &mut impl FnMut(&ContainerID) -> usize,
    register_peer: &mut impl FnMut(u64) -> usize,
) -> (i32, ValueKind) {
    let (prop, value_type) = match &op.content {
        crate::op::InnerContent::List(list) => match list {
            crate::container::list::list_op::InnerListOp::Insert { slice, pos } => {
                assert_eq!(op.container.get_type(), ContainerType::List);
                let value = arena.get_values(slice.0.start as usize..slice.0.end as usize);
                value_writer.write_value_content(&value.into(), register_key, register_cid);
                (*pos as i32, ValueKind::Array)
            }
            crate::container::list::list_op::InnerListOp::InsertText {
                slice,
                unicode_start: _,
                unicode_len: _,
                pos,
            } => {
                // TODO: refactor this from_utf8 can be done internally without checking
                value_writer.write(
                    &value::Value::Str(std::str::from_utf8(slice.as_bytes()).unwrap()),
                    register_key,
                    register_cid,
                );
                (*pos as i32, ValueKind::Str)
            }
            crate::container::list::list_op::InnerListOp::Delete(span) => {
                value_writer.write(
                    &value::Value::DeleteSeq(span.signed_len as i32),
                    register_key,
                    register_cid,
                );
                (span.pos as i32, ValueKind::DeleteSeq)
            }
            crate::container::list::list_op::InnerListOp::StyleStart {
                start,
                end,
                key,
                value,
                info,
            } => {
                value_writer.write(
                    &value::Value::MarkStart(MarkStart {
                        len: end - start,
                        key_idx: register_key(key) as u32,
                        value: value.clone(),
                        info: info.to_byte(),
                    }),
                    register_key,
                    register_cid,
                );
                (*start as i32, ValueKind::MarkStart)
            }
            crate::container::list::list_op::InnerListOp::StyleEnd => (0, ValueKind::Null),
        },
        crate::op::InnerContent::Map(map) => {
            assert_eq!(op.container.get_type(), ContainerType::Map);
            let key = register_key(&map.key);
            match &map.value {
                Some(v) => {
                    let kind = value_writer.write_value_content(v, register_key, register_cid);
                    (key as i32, kind)
                }
                None => (key as i32, ValueKind::DeleteOnce),
            }
        }
        crate::op::InnerContent::Tree(t) => {
            assert_eq!(op.container.get_type(), ContainerType::Tree);
            let op = EncodedTreeMove::from_tree_op(t, register_peer);
            value_writer.write(&value::Value::TreeMove(op), register_key, register_cid);
            (0, ValueKind::TreeMove)
        }
    };
    (prop, value_type)
}

#[inline]
fn decode_op(
    cid: &ContainerID,
    kind: ValueKind,
    value_reader: &mut ValueReader<'_>,
    arena: &crate::arena::SharedArena,
    prop: i32,
    keys: &arena::KeyArena,
    peers: &[u64],
    cids: &[ContainerID],
) -> crate::op::InnerContent {
    let content = match cid.container_type() {
        ContainerType::Text => match kind {
            ValueKind::Str => {
                let s = value_reader.read_str();
                let (slice, result) = arena.alloc_str_with_slice(s);
                crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::InsertText {
                        slice,
                        unicode_start: result.start as u32,
                        unicode_len: (result.end - result.start) as u32,
                        pos: prop as u32,
                    },
                )
            }
            ValueKind::DeleteSeq => {
                let len = value_reader.read_i32();
                crate::op::InnerContent::List(crate::container::list::list_op::InnerListOp::Delete(
                    DeleteSpan::new(prop as isize, len as isize),
                ))
            }
            ValueKind::MarkStart => {
                let mark = value_reader.read_mark(&keys.keys, &cids);
                let key = keys.keys[mark.key_idx as usize].clone();
                crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::StyleStart {
                        start: prop as u32,
                        end: prop as u32 + mark.len,
                        key,
                        value: mark.value,
                        info: TextStyleInfoFlag::from_byte(mark.info),
                    },
                )
            }
            ValueKind::Null => crate::op::InnerContent::List(
                crate::container::list::list_op::InnerListOp::StyleEnd,
            ),
            _ => unreachable!(),
        },
        ContainerType::Map => {
            let key = keys.keys[prop as usize].clone();
            match kind {
                ValueKind::DeleteOnce => {
                    crate::op::InnerContent::Map(crate::container::map::MapSet { key, value: None })
                }
                kind => {
                    let value = value_reader.read_value_content(kind, &keys.keys, &cids);
                    crate::op::InnerContent::Map(crate::container::map::MapSet {
                        key,
                        value: Some(value),
                    })
                }
            }
        }
        ContainerType::List => {
            let pos = prop as usize;
            match kind {
                ValueKind::Array => {
                    let arr = value_reader.read_value_content(ValueKind::Array, &keys.keys, &cids);
                    let range = arena.alloc_values(
                        Arc::try_unwrap(arr.into_list().unwrap())
                            .unwrap()
                            .into_iter(),
                    );
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Insert {
                            slice: SliceRange::new(range.start as u32..range.end as u32),
                            pos,
                        },
                    )
                }
                ValueKind::DeleteSeq => {
                    let len = value_reader.read_i32();
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Delete(DeleteSpan::new(
                            pos as isize,
                            len as isize,
                        )),
                    )
                }
                _ => unreachable!(),
            }
        }
        ContainerType::Tree => match kind {
            ValueKind::TreeMove => {
                let op = value_reader.read_tree_move();
                crate::op::InnerContent::Tree(op.into_tree_op(peers))
            }
            _ => unreachable!(),
        },
    };
    content
}

type PeerIdx = usize;

/// Extract containers from oplog changes.
///
/// Containers are sorted by their peer_id and counter so that
/// they can be compressed by using delta encoding.
fn extract_containers_in_order(
    diff_changes: &Vec<Cow<Change>>,
    oplog: &OpLog,
) -> (Vec<ContainerID>, FxHashMap<ContainerIdx, usize>) {
    let mut containers = Vec::new();
    let mut visited = FxHashSet::default();
    for change in diff_changes {
        for op in change.ops.iter() {
            let container = op.container;
            if visited.contains(&container) {
                continue;
            }

            visited.insert(container);
            let id = oplog.arena.get_container_id(container).unwrap();
            containers.push((id, container));
        }
    }

    containers.sort_unstable_by(|(a, _), (b, _)| {
        a.is_root()
            .cmp(&b.is_root())
            .then_with(|| a.container_type().cmp(&b.container_type()))
            .then_with(|| match (a, b) {
                (ContainerID::Root { name: a, .. }, ContainerID::Root { name: b, .. }) => a.cmp(b),
                (
                    ContainerID::Normal {
                        peer: peer_a,
                        counter: counter_a,
                        ..
                    },
                    ContainerID::Normal {
                        peer: peer_b,
                        counter: counter_b,
                        ..
                    },
                ) => peer_a.cmp(peer_b).then_with(|| counter_a.cmp(counter_b)),
                _ => unreachable!(),
            })
    });

    let container_idx2index = containers
        .iter()
        .enumerate()
        .map(|(i, (_, c))| (*c, i))
        .collect();

    (
        containers.into_iter().map(|x| x.0).collect(),
        container_idx2index,
    )
}

#[columnar(ser, de)]
struct EncodedDoc<'a> {
    #[columnar(class = "vec", iter = "EncodedOp")]
    ops: Vec<EncodedOp>,
    #[columnar(class = "vec", iter = "EncodedChange")]
    changes: Vec<EncodedChange>,

    #[columnar(borrow)]
    raw_values: Cow<'a, [u8]>,

    /// A list of encoded arenas, in the following order
    /// - `peer_id_arena`
    /// - `container_arena`
    /// - `key_arena`
    /// - `deps_arena`
    /// - `others`, left for future use
    #[columnar(borrow)]
    arenas: Cow<'a, [u8]>,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedOp {
    #[columnar(strategy = "DeltaRle")]
    container_index: u32,
    #[columnar(strategy = "DeltaRle")]
    prop: i32,
    #[columnar(strategy = "Rle")]
    peer_idx: u32,
    #[columnar(strategy = "DeltaRle")]
    value_type: u8,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedChange {
    #[columnar(strategy = "Rle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
    #[columnar(strategy = "DeltaRle")]
    lamport: u32,
    #[columnar(strategy = "DeltaRle")]
    len: usize,
    #[columnar(strategy = "DeltaRle")]
    timestamp: i64,
    #[columnar(strategy = "DeltaRle")]
    deps_len: i32,
    #[columnar(strategy = "BoolRle")]
    dep_on_self: bool,
    #[columnar(strategy = "DeltaRle")]
    msg_len: i32,
}

mod value {
    use std::sync::Arc;

    use fxhash::FxHashMap;
    use loro_common::{ContainerID, Counter, InternalString, LoroValue, TreeID};
    use num_derive::{FromPrimitive, ToPrimitive};
    use num_traits::{FromPrimitive, ToPrimitive};

    use crate::container::tree::tree_op::TreeOp;

    #[allow(unused)]
    #[non_exhaustive]
    pub enum Value<'a> {
        Null,
        True,
        False,
        DeleteOnce,
        ContainerIdx(usize),
        I32(i32),
        F64(f64),
        Str(&'a str),
        DeleteSeq(i32),
        DeltaInt(i32),
        Array(Vec<Value<'a>>),
        Map(FxHashMap<InternalString, Value<'a>>),
        Binary(&'a [u8]),
        MarkStart(MarkStart),
        TreeMove(EncodedTreeMove),
        Unknown { kind: u8, data: &'a [u8] },
    }

    pub struct MarkStart {
        pub len: u32,
        pub key_idx: u32,
        pub value: LoroValue,
        pub info: u8,
    }

    pub struct EncodedTreeMove {
        pub subject_peer_idx: usize,
        pub subject_cnt: usize,
        pub is_parent_null: bool,
        pub parent_peer_idx: usize,
        pub parent_cnt: usize,
    }

    impl EncodedTreeMove {
        pub fn into_tree_op(&self, peer_ids: &[u64]) -> TreeOp {
            TreeOp {
                target: TreeID::new(peer_ids[self.subject_peer_idx], self.subject_cnt as Counter),
                parent: if self.is_parent_null {
                    None
                } else {
                    Some(TreeID::new(
                        peer_ids[self.parent_peer_idx],
                        self.parent_cnt as Counter,
                    ))
                },
            }
        }

        pub fn from_tree_op(op: &TreeOp, register_peer_id: &mut dyn FnMut(u64) -> usize) -> Self {
            EncodedTreeMove {
                subject_peer_idx: register_peer_id(op.target.peer),
                subject_cnt: op.target.counter as usize,
                is_parent_null: op.parent.is_none(),
                parent_peer_idx: op.parent.map_or(0, |x| register_peer_id(x.peer)),
                parent_cnt: op.parent.map_or(0, |x| x.counter as usize),
            }
        }
    }

    #[non_exhaustive]
    #[derive(Debug, FromPrimitive, ToPrimitive)]
    pub enum ValueKind {
        Null = 0,
        True = 1,
        False = 2,
        DeleteOnce = 3,
        I32 = 4,
        ContainerIdx = 5,
        F64 = 6,
        Str = 7,
        DeleteSeq = 8,
        DeltaInt = 9,
        Array = 10,
        Map = 11,
        MarkStart = 12,
        TreeMove = 13,
        Binary = 14,
        Unknown = 65536,
    }

    impl<'a> Value<'a> {
        pub fn kind(&self) -> ValueKind {
            match self {
                Value::Null => ValueKind::Null,
                Value::True => ValueKind::True,
                Value::False => ValueKind::False,
                Value::DeleteOnce => ValueKind::DeleteOnce,
                Value::I32(_) => ValueKind::I32,
                Value::ContainerIdx(_) => ValueKind::ContainerIdx,
                Value::F64(_) => ValueKind::F64,
                Value::Str(_) => ValueKind::Str,
                Value::DeleteSeq(_) => ValueKind::DeleteSeq,
                Value::DeltaInt(_) => ValueKind::DeltaInt,
                Value::Array(_) => ValueKind::Array,
                Value::Map(_) => ValueKind::Map,
                Value::MarkStart { .. } => ValueKind::MarkStart,
                Value::TreeMove(_) => ValueKind::TreeMove,
                Value::Binary(_) => ValueKind::Binary,
                Value::Unknown { .. } => ValueKind::Unknown,
            }
        }
    }

    fn get_loro_value_kind(value: &LoroValue) -> ValueKind {
        match value {
            LoroValue::Null => ValueKind::Null,
            LoroValue::Bool(true) => ValueKind::True,
            LoroValue::Bool(false) => ValueKind::False,
            LoroValue::I32(_) => ValueKind::I32,
            LoroValue::Double(_) => ValueKind::F64,
            LoroValue::String(_) => ValueKind::Str,
            LoroValue::List(_) => ValueKind::Array,
            LoroValue::Map(_) => ValueKind::Map,
            LoroValue::Binary(_) => ValueKind::Binary,
            LoroValue::Container(_) => ValueKind::ContainerIdx,
        }
    }

    pub struct ValueWriter {
        buffer: Vec<u8>,
    }

    impl ValueWriter {
        pub fn new() -> Self {
            ValueWriter { buffer: Vec::new() }
        }

        pub fn write_value_type_and_content(
            &mut self,
            value: &LoroValue,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
            register_cid: &mut dyn FnMut(&ContainerID) -> usize,
        ) -> ValueKind {
            self.write_u8(get_loro_value_kind(value).to_u8().unwrap());
            self.write_value_content(value, register_key, register_cid)
        }

        pub fn write_value_content(
            &mut self,
            value: &LoroValue,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
            register_cid: &mut dyn FnMut(&ContainerID) -> usize,
        ) -> ValueKind {
            match value {
                LoroValue::Null => ValueKind::Null,
                LoroValue::Bool(true) => ValueKind::True,
                LoroValue::Bool(false) => ValueKind::False,
                LoroValue::I32(value) => {
                    self.write_i32(*value);
                    ValueKind::I32
                }
                LoroValue::Double(value) => {
                    self.write_f64(*value);
                    ValueKind::F64
                }
                LoroValue::String(value) => {
                    self.write_str(value);
                    ValueKind::Str
                }
                LoroValue::List(value) => {
                    self.write_usize(value.len());
                    for value in value.iter() {
                        self.write_value_type_and_content(value, register_key, register_cid);
                    }
                    ValueKind::Array
                }
                LoroValue::Map(value) => {
                    self.write_usize(value.len());
                    for (key, value) in value.iter() {
                        let key_idx = register_key(&key.as_str().into());
                        self.write_usize(key_idx);
                        self.write_kind(get_loro_value_kind(value));
                        self.write_value_type_and_content(value, register_key, register_cid);
                    }
                    ValueKind::Map
                }
                LoroValue::Binary(value) => {
                    self.write_binary(value);
                    ValueKind::Binary
                }
                LoroValue::Container(c) => {
                    let idx = register_cid(c);
                    self.write_usize(idx);
                    ValueKind::ContainerIdx
                }
            }
        }

        pub fn write(
            &mut self,
            value: &Value,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
            register_cid: &mut dyn FnMut(&ContainerID) -> usize,
        ) {
            match value {
                Value::Null => {}
                Value::True => {}
                Value::False => {}
                Value::DeleteOnce => {}
                Value::I32(value) => self.write_i32(*value),
                Value::F64(value) => self.write_f64(*value),
                Value::Str(value) => self.write_str(value),
                Value::DeleteSeq(value) => self.write_i32(*value),
                Value::DeltaInt(value) => self.write_i32(*value),
                Value::Array(value) => self.write_array(value, register_key, register_cid),
                Value::Map(value) => self.write_map(value, register_key, register_cid),
                Value::MarkStart(value) => self.write_mark(value, register_key, register_cid),
                Value::TreeMove(op) => self.write_tree_move(op),
                Value::Binary(value) => self.write_binary(value),
                Value::ContainerIdx(value) => self.write_usize(*value),
                Value::Unknown { kind: _, data } => self.write_binary(data),
            }
        }

        fn write_i32(&mut self, value: i32) {
            leb128::write::signed(&mut self.buffer, value as i64).unwrap();
        }

        fn write_usize(&mut self, value: usize) {
            leb128::write::unsigned(&mut self.buffer, value as u64).unwrap();
        }

        fn write_f64(&mut self, value: f64) {
            self.buffer.extend_from_slice(&value.to_be_bytes());
        }

        fn write_str(&mut self, value: &str) {
            self.write_usize(value.len());
            self.buffer.extend_from_slice(value.as_bytes());
        }

        fn write_u8(&mut self, value: u8) {
            self.buffer.push(value);
        }

        pub fn write_kind(&mut self, kind: ValueKind) {
            self.write_u8(kind.to_u8().unwrap());
        }

        fn write_array(
            &mut self,
            value: &[Value],
            register_key: &mut dyn FnMut(&InternalString) -> usize,
            register_cid: &mut dyn FnMut(&ContainerID) -> usize,
        ) {
            self.write_usize(value.len());
            for value in value {
                self.write_kind(value.kind());
                self.write(value, register_key, register_cid);
            }
        }

        fn write_map(
            &mut self,
            value: &FxHashMap<InternalString, Value>,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
            register_cid: &mut dyn FnMut(&ContainerID) -> usize,
        ) {
            self.write_usize(value.len());
            for (key, value) in value {
                let key_idx = register_key(key);
                self.write_usize(key_idx);
                self.write_kind(value.kind());
                self.write(value, register_key, register_cid);
            }
        }

        fn write_binary(&mut self, value: &[u8]) {
            self.write_usize(value.len());
            self.buffer.extend_from_slice(value);
        }

        fn write_mark(
            &mut self,
            mark: &MarkStart,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
            register_cid: &mut dyn FnMut(&ContainerID) -> usize,
        ) {
            self.write_u8(mark.info);
            self.write_usize(mark.len as usize);
            self.write_usize(mark.key_idx as usize);
            self.write_value_type_and_content(&mark.value, register_key, register_cid);
        }

        fn write_tree_move(&mut self, op: &EncodedTreeMove) {
            self.write_usize(op.subject_peer_idx);
            self.write_usize(op.subject_cnt);
            self.write_u8(op.is_parent_null as u8);
            self.write_usize(op.parent_peer_idx);
            self.write_usize(op.parent_cnt);
        }

        pub(crate) fn finish(self) -> Vec<u8> {
            self.buffer
        }
    }

    pub struct ValueReader<'a> {
        raw: &'a [u8],
    }

    impl<'a> ValueReader<'a> {
        pub fn new(raw: &'a [u8]) -> Self {
            ValueReader { raw }
        }

        pub fn read(
            &mut self,
            kind: u8,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> Value<'a> {
            let Some(kind) = ValueKind::from_u8(kind) else {
                return Value::Unknown {
                    kind,
                    data: self.read_binary(),
                };
            };

            match kind {
                ValueKind::Null => Value::Null,
                ValueKind::True => Value::True,
                ValueKind::False => Value::False,
                ValueKind::DeleteOnce => Value::DeleteOnce,
                ValueKind::I32 => Value::I32(self.read_i32()),
                ValueKind::F64 => Value::F64(self.read_f64()),
                ValueKind::Str => Value::Str(self.read_str()),
                ValueKind::DeleteSeq => Value::DeleteSeq(self.read_i32()),
                ValueKind::DeltaInt => Value::DeltaInt(self.read_i32()),
                ValueKind::Array => Value::Array(self.read_array(keys, cids)),
                ValueKind::Map => Value::Map(self.read_map(keys, cids)),
                ValueKind::Binary => Value::Binary(self.read_binary()),
                ValueKind::MarkStart => Value::MarkStart(self.read_mark(keys, cids)),
                ValueKind::TreeMove => Value::TreeMove(self.read_tree_move()),
                ValueKind::ContainerIdx => Value::ContainerIdx(self.read_usize()),
                ValueKind::Unknown => unreachable!(),
            }
        }

        pub fn read_value_type_and_content(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroValue {
            let kind = self.read_u8();
            self.read_value_content(ValueKind::from_u8(kind).unwrap(), keys, cids)
        }

        pub fn read_value_content(
            &mut self,
            kind: ValueKind,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroValue {
            match kind {
                ValueKind::Null => LoroValue::Null,
                ValueKind::True => LoroValue::Bool(true),
                ValueKind::False => LoroValue::Bool(false),
                ValueKind::I32 => LoroValue::I32(self.read_i32()),
                ValueKind::F64 => LoroValue::Double(self.read_f64()),
                ValueKind::Str => LoroValue::String(Arc::new(self.read_str().to_owned())),
                ValueKind::DeltaInt => LoroValue::I32(self.read_i32()),
                ValueKind::Array => {
                    let len = self.read_usize();
                    let mut ans = Vec::with_capacity(len);
                    for _ in 0..len {
                        ans.push(self.read_value_type_and_content(keys, cids));
                    }
                    ans.into()
                }
                ValueKind::Map => {
                    let len = self.read_usize();
                    let mut ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
                    for _ in 0..len {
                        let key_idx = self.read_usize();
                        let key = keys[key_idx].to_string();
                        let value = self.read_value_type_and_content(keys, cids);
                        ans.insert(key, value);
                    }
                    ans.into()
                }
                ValueKind::Binary => LoroValue::Binary(Arc::new(self.read_binary().to_owned())),
                ValueKind::ContainerIdx => LoroValue::Container(cids[self.read_usize()].clone()),
                a => unreachable!("Unexpected value kind {:?}", a),
            }
        }

        pub fn read_i32(&mut self) -> i32 {
            leb128::read::signed(&mut self.raw).unwrap() as i32
        }

        fn read_f64(&mut self) -> f64 {
            let mut bytes = [0; 8];
            bytes.copy_from_slice(&self.raw[..8]);
            self.raw = &self.raw[8..];
            f64::from_be_bytes(bytes)
        }

        pub fn read_usize(&mut self) -> usize {
            leb128::read::unsigned(&mut self.raw).unwrap() as usize
        }

        pub fn read_str(&mut self) -> &'a str {
            let len = self.read_usize();
            let ans = std::str::from_utf8(&self.raw[..len as usize]).unwrap();
            self.raw = &self.raw[len as usize..];
            ans
        }

        fn read_u8(&mut self) -> u8 {
            let ans = self.raw[0];
            self.raw = &self.raw[1..];
            ans
        }

        fn read_kind(&mut self) -> ValueKind {
            ValueKind::from_u8(self.read_u8()).unwrap_or(ValueKind::Unknown)
        }

        fn read_array(&mut self, keys: &[InternalString], cids: &[ContainerID]) -> Vec<Value<'a>> {
            let len = self.read_usize();
            let mut ans = Vec::with_capacity(len);
            for _ in 0..len {
                let kind = self.read_u8();
                ans.push(self.read(kind, keys, cids));
            }
            ans
        }

        fn read_map(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> FxHashMap<InternalString, Value<'a>> {
            let len = self.read_usize();
            let mut ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
            for _ in 0..len {
                let key_idx = self.read_usize();
                let key = keys[key_idx].clone();
                let kind = self.read_u8();
                let value = self.read(kind, keys, cids);
                ans.insert(key, value);
            }
            ans
        }

        fn read_binary(&mut self) -> &'a [u8] {
            let len = self.read_usize();
            let ans = &self.raw[..len];
            self.raw = &self.raw[len..];
            ans
        }

        fn read_unknown(&mut self) -> &'a [u8] {
            let len = self.read_usize();
            let ans = &self.raw[..len];
            self.raw = &self.raw[len..];
            ans
        }

        pub fn read_mark(&mut self, keys: &[InternalString], cids: &[ContainerID]) -> MarkStart {
            let info = self.read_u8();
            let len = self.read_usize();
            let key_idx = self.read_usize();
            let value = self.read_value_type_and_content(keys, cids);
            MarkStart {
                len: len as u32,
                key_idx: key_idx as u32,
                value,
                info,
            }
        }

        pub fn read_tree_move(&mut self) -> EncodedTreeMove {
            let subject_peer_idx = self.read_usize();
            let subject_cnt = self.read_usize();
            let is_parent_null = self.read_u8() != 0;
            let parent_peer_idx = self.read_usize();
            let parent_cnt = self.read_usize();
            EncodedTreeMove {
                subject_peer_idx,
                subject_cnt,
                is_parent_null,
                parent_peer_idx,
                parent_cnt,
            }
        }
    }
}

mod arena {
    use crate::InternalString;
    use loro_common::{ContainerID, ContainerType, LoroResult, PeerID, ID};
    use serde::{Deserialize, Serialize};
    use serde_columnar::columnar;

    use super::PeerIdx;

    pub fn encode_arena(
        peer_ids_arena: Vec<u64>,
        containers: ContainerArena,
        keys: Vec<InternalString>,
        deps: DepsArena,
    ) -> Vec<u8> {
        let peer_ids = PeerIdArena {
            peer_ids: peer_ids_arena,
        };

        let key_arena = KeyArena { keys };
        let encoded = EncodedArenas {
            peer_id_arena: &peer_ids.encode(),
            container_arena: &containers.encode(),
            key_arena: &key_arena.encode(),
            deps_arena: &deps.encode(),
        };

        encoded.encode_arenas()
    }

    pub struct DecodedArenas<'a> {
        pub peer_ids: PeerIdArena,
        pub containers: ContainerArena,
        pub keys: KeyArena,
        pub deps: Box<dyn Iterator<Item = EncodedDep> + 'a>,
    }

    pub fn decode_arena(bytes: &[u8]) -> LoroResult<DecodedArenas> {
        let arenas = EncodedArenas::decode_arenas(bytes)?;
        Ok(DecodedArenas {
            peer_ids: PeerIdArena::decode(arenas.peer_id_arena)?,
            containers: ContainerArena::decode(arenas.container_arena)?,
            keys: KeyArena::decode(arenas.key_arena)?,
            deps: Box::new(DepsArena::decode_iter(arenas.deps_arena)?),
        })
    }

    struct EncodedArenas<'a> {
        peer_id_arena: &'a [u8],
        container_arena: &'a [u8],
        key_arena: &'a [u8],
        deps_arena: &'a [u8],
    }

    impl EncodedArenas<'_> {
        fn encode_arenas(self) -> Vec<u8> {
            let mut ans = Vec::with_capacity(
                self.peer_id_arena.len()
                    + self.container_arena.len()
                    + self.key_arena.len()
                    + self.deps_arena.len()
                    + 4 * 4,
            );

            write_arena(&mut ans, self.peer_id_arena);
            write_arena(&mut ans, self.container_arena);
            write_arena(&mut ans, self.key_arena);
            write_arena(&mut ans, self.deps_arena);
            ans
        }

        fn decode_arenas(bytes: &[u8]) -> LoroResult<EncodedArenas> {
            let (peer_id_arena, rest) = read_arena(bytes);
            let (container_arena, rest) = read_arena(rest);
            let (key_arena, rest) = read_arena(rest);
            let (deps_arena, _) = read_arena(rest);
            Ok(EncodedArenas {
                peer_id_arena,
                container_arena,
                key_arena,
                deps_arena,
            })
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct PeerIdArena {
        pub(super) peer_ids: Vec<u64>,
    }

    impl PeerIdArena {
        fn encode(&self) -> Vec<u8> {
            let mut ans = Vec::with_capacity(self.peer_ids.len() * 8);
            leb128::write::unsigned(&mut ans, self.peer_ids.len() as u64).unwrap();
            for &peer_id in &self.peer_ids {
                ans.extend_from_slice(&peer_id.to_be_bytes());
            }
            ans
        }

        fn decode(peer_id_arena: &[u8]) -> LoroResult<Self> {
            let mut reader = peer_id_arena;
            let len = leb128::read::unsigned(&mut reader).unwrap();
            let mut peer_ids = Vec::with_capacity(len as usize);
            for _ in 0..len {
                let mut peer_id_bytes = [0; 8];
                peer_id_bytes.copy_from_slice(&reader[..8]);
                peer_ids.push(u64::from_be_bytes(peer_id_bytes));
                reader = &reader[8..];
            }
            Ok(PeerIdArena { peer_ids })
        }
    }

    #[columnar(vec, ser, de, iterable)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub(super) struct EncodedContainer {
        #[columnar(strategy = "BoolRle")]
        is_root: bool,
        #[columnar(strategy = "Rle")]
        kind: ContainerType,
        #[columnar(strategy = "Rle")]
        peer_idx: usize,
        #[columnar(strategy = "DeltaRle")]
        key_idx_or_counter: i32,
    }

    impl EncodedContainer {
        pub fn to_container_id(
            &self,
            key_arena: &[InternalString],
            peer_arena: &[u64],
        ) -> ContainerID {
            if self.is_root {
                ContainerID::Root {
                    container_type: self.kind,
                    name: key_arena[self.key_idx_or_counter as usize].clone(),
                }
            } else {
                ContainerID::Normal {
                    container_type: self.kind,
                    peer: peer_arena[self.peer_idx],
                    counter: self.key_idx_or_counter,
                }
            }
        }
    }

    #[columnar(ser, de)]
    #[derive(Default)]
    pub(super) struct ContainerArena {
        #[columnar(class = "vec", iter = "EncodedContainer")]
        pub(super) containers: Vec<EncodedContainer>,
    }

    impl ContainerArena {
        fn encode(&self) -> Vec<u8> {
            serde_columnar::to_vec(&self.containers).unwrap()
        }

        fn decode(bytes: &[u8]) -> LoroResult<Self> {
            Ok(ContainerArena {
                containers: serde_columnar::from_bytes(bytes)?,
            })
        }

        pub fn from_containers(
            cids: Vec<ContainerID>,
            register_peer_id: &mut dyn FnMut(PeerID) -> usize,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
        ) -> Self {
            let mut ans = Self {
                containers: Vec::with_capacity(cids.len()),
            };
            for cid in cids {
                ans.push(cid, register_peer_id, register_key);
            }

            ans
        }

        pub fn push(
            &mut self,
            id: ContainerID,
            register_peer_id: &mut dyn FnMut(PeerID) -> usize,
            register_key: &mut dyn FnMut(&InternalString) -> usize,
        ) {
            let (is_root, kind, peer_idx, key_idx_or_counter) = match id {
                ContainerID::Root {
                    container_type,
                    name,
                } => (true, container_type, 0, register_key(&name) as i32),
                ContainerID::Normal {
                    container_type,
                    peer,
                    counter,
                } => (false, container_type, register_peer_id(peer), counter),
            };
            self.containers.push(EncodedContainer {
                is_root,
                kind,
                peer_idx,
                key_idx_or_counter,
            });
        }
    }

    #[columnar(vec, ser, de, iterable)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct EncodedDep {
        #[columnar(strategy = "Rle")]
        pub peer_idx: usize,
        #[columnar(strategy = "DeltaRle")]
        pub counter: i32,
    }

    #[columnar(ser, de)]
    #[derive(Default)]
    pub(super) struct DepsArena {
        #[columnar(class = "vec", iter = "EncodedDep")]
        deps: Vec<EncodedDep>,
    }

    impl DepsArena {
        pub fn push(&mut self, peer_idx: PeerIdx, counter: i32) {
            self.deps.push(EncodedDep { peer_idx, counter });
        }

        pub fn iter<'a>(&'a self, peer_arenas: &'a [PeerID]) -> impl Iterator<Item = ID> + 'a {
            self.deps
                .iter()
                .map(|dep| ID::new(peer_arenas[dep.peer_idx], dep.counter))
        }

        pub fn encode(&self) -> Vec<u8> {
            serde_columnar::to_vec(&self).unwrap()
        }

        pub fn decode_iter<'a>(
            bytes: &'a [u8],
        ) -> LoroResult<impl Iterator<Item = EncodedDep> + 'a> {
            let iter = serde_columnar::iter_from_bytes::<DepsArena>(bytes)?;
            Ok(iter.deps)
        }
    }

    #[derive(Serialize, Deserialize, Default)]
    pub(super) struct KeyArena {
        pub(super) keys: Vec<InternalString>,
    }

    impl KeyArena {
        pub fn encode(&self) -> Vec<u8> {
            serde_columnar::to_vec(&self).unwrap()
        }

        pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
            Ok(serde_columnar::from_bytes(bytes)?)
        }
    }

    fn write_arena(buffer: &mut Vec<u8>, arena: &[u8]) {
        leb128::write::unsigned(buffer, arena.len() as u64).unwrap();
        buffer.extend_from_slice(arena);
    }

    /// Return (next_arena, rest)
    fn read_arena(mut buffer: &[u8]) -> (&[u8], &[u8]) {
        let reader = &mut buffer;
        let len = leb128::read::unsigned(reader).unwrap();
        (reader[..len as usize].as_ref(), &reader[len as usize..])
    }
}
