pub(crate) use encode::{encode_op, get_op_prop};
use fractional_index::FractionalIndex;
use loro_common::{
    ContainerID, ContainerType, Counter, HasCounterSpan, HasIdSpan, IdLp, LoroError, LoroResult,
    TreeID, ID,
};
use rustc_hash::FxHashSet;
use serde_columnar::{columnar, ColumnarError};
use std::borrow::Cow;
use std::sync::Arc;

use crate::version::VersionRange;
use crate::{
    arena::SharedArena,
    change::{Change, Lamport, Timestamp},
    container::{
        list::list_op::DeleteSpanWithId, richtext::TextStyleInfoFlag, tree::tree_op::TreeOp,
    },
    op::{FutureInnerContent, Op, SliceRange},
    OpLog, VersionVector,
};

use super::ParsedHeaderAndBody;
use super::{
    arena::*,
    value::{Value, ValueDecodedArenasTrait, ValueReader, ValueWriter},
    ImportBlobMetadata,
};

pub(crate) use crate::encoding::value_register::ValueRegister;

#[allow(unused_imports)]
use super::value::FutureValue;

/// If any section of the document is longer than this, we will not decode it.
/// It will return an data corruption error instead.
pub(super) const MAX_DECODED_SIZE: usize = 1 << 30;
/// If any collection in the document is longer than this, we will not decode it.
/// It will return an data corruption error instead.
pub(super) const MAX_COLLECTION_SIZE: usize = 1 << 28;

pub fn decode_import_blob_meta(parsed: ParsedHeaderAndBody) -> LoroResult<ImportBlobMetadata> {
    let iterators = serde_columnar::iter_from_bytes::<EncodedDoc>(parsed.body)?;
    let DecodedArenas { peer_ids, .. } = decode_arena(&iterators.arenas)?;
    let start_vv: VersionVector = iterators
        .start_counters
        .iter()
        .enumerate()
        .filter_map(|(peer_idx, counter)| {
            if *counter == 0 {
                None
            } else {
                Some(ID::new(peer_ids.peer_ids[peer_idx], *counter - 1))
            }
        })
        .collect();
    let frontiers = iterators
        .start_frontiers
        .iter()
        .map(|x| ID::new(peer_ids.peer_ids[x.0], x.1))
        .collect();
    let mut end_vv_counters = iterators.start_counters;
    let mut change_num = 0;
    let mut start_timestamp = Timestamp::MAX;
    let mut end_timestamp = Timestamp::MIN;

    for iter in iterators.changes {
        let EncodedChange {
            peer_idx,
            len,
            timestamp,
            ..
        } = iter?;
        end_vv_counters[peer_idx] += len as Counter;
        start_timestamp = start_timestamp.min(timestamp);
        end_timestamp = end_timestamp.max(timestamp);
        change_num += 1;
    }

    Ok(ImportBlobMetadata {
        mode: match parsed.mode {
            super::EncodeMode::OutdatedRle => super::EncodedBlobMode::OutdatedRle,
            super::EncodeMode::OutdatedSnapshot => super::EncodedBlobMode::OutdatedSnapshot,
            super::EncodeMode::FastSnapshot => super::EncodedBlobMode::Snapshot,
            super::EncodeMode::FastUpdates => super::EncodedBlobMode::Updates,
            super::EncodeMode::Auto => unreachable!(),
        },
        start_frontiers: frontiers,
        partial_start_vv: start_vv,
        partial_end_vv: VersionVector::from_iter(
            end_vv_counters
                .iter()
                .enumerate()
                .map(|(peer_idx, counter)| ID::new(peer_ids.peer_ids[peer_idx], *counter - 1)),
        ),
        start_timestamp,
        end_timestamp,
        change_num,
    })
}

pub(crate) struct ImportChangesResult {
    pub latest_ids: Vec<ID>,
    pub pending_changes: Vec<Change>,
    pub changes_that_have_deps_before_shallow_root: Vec<Change>,
    pub imported: VersionRange,
}

/// NOTE: This method expects that the remote_changes are already sorted by lamport value
pub(crate) fn import_changes_to_oplog(
    changes: Vec<Change>,
    oplog: &mut OpLog,
) -> ImportChangesResult {
    let mut pending_changes = Vec::new();
    let mut latest_ids = Vec::new();
    let mut changes_before_shallow_root = Vec::new();
    let mut imported = VersionRange::default();
    for mut change in changes {
        if change.ctr_end() <= oplog.vv().get(&change.id.peer).copied().unwrap_or(0) {
            // skip included changes
            continue;
        }

        if oplog.dag.is_before_shallow_root(&change.deps) {
            changes_before_shallow_root.push(change);
            continue;
        }

        latest_ids.push(change.id_last());
        // calc lamport or pending if its deps are not satisfied
        match oplog.dag.get_change_lamport_from_deps(&change.deps) {
            Some(lamport) => change.lamport = lamport,
            None => {
                pending_changes.push(change);
                continue;
            }
        }

        let Some(change) = oplog.trim_the_known_part_of_change(change) else {
            continue;
        };

        imported.extends_to_include_id_span(change.id_span());
        oplog.insert_new_change(change, false);
    }

    ImportChangesResult {
        latest_ids,
        pending_changes,
        changes_that_have_deps_before_shallow_root: changes_before_shallow_root,
        imported,
    }
}

mod encode {
    use loro_common::ContainerType;

    use crate::{
        arena::SharedArena,
        encoding::value::{MarkStart, Value, ValueEncodeRegister, ValueKind, ValueWriter},
        op::{FutureInnerContent, Op},
    };

    use super::EncodedDeleteStartId;

    fn get_future_op_prop(op: &FutureInnerContent) -> i32 {
        match op {
            #[cfg(feature = "counter")]
            FutureInnerContent::Counter(_) => 0,
            FutureInnerContent::Unknown { prop, .. } => *prop,
        }
    }

    pub(crate) fn get_op_prop(op: &Op, registers: &mut dyn ValueEncodeRegister) -> i32 {
        match &op.content {
            crate::op::InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Move { to, .. } => *to as i32,
                crate::container::list::list_op::InnerListOp::Set { .. } => 0,
                crate::container::list::list_op::InnerListOp::Insert { pos, .. } => *pos as i32,
                crate::container::list::list_op::InnerListOp::InsertText { pos, .. } => *pos as i32,
                crate::container::list::list_op::InnerListOp::Delete(span) => span.span.pos as i32,
                crate::container::list::list_op::InnerListOp::StyleStart { start, .. } => {
                    *start as i32
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => 0,
            },
            crate::op::InnerContent::Map(map) => {
                let key = registers.key_mut().register(&map.key);
                key as i32
            }
            crate::op::InnerContent::Tree(_) => 0,
            crate::op::InnerContent::Future(f) => get_future_op_prop(f),
        }
    }

    #[inline]
    pub(crate) fn encode_op<'p, 'a: 'p>(
        op: &'a Op,
        arena: &SharedArena,
        delete_start: &mut Vec<EncodedDeleteStartId>,
        value_writer: &mut ValueWriter,
        registers: &mut dyn ValueEncodeRegister,
    ) -> ValueKind {
        let value = match &op.content {
            crate::op::InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Insert { slice, .. } => {
                    assert!(matches!(
                        op.container.get_type(),
                        ContainerType::List | ContainerType::MovableList
                    ));
                    let value = arena.get_values(slice.0.start as usize..slice.0.end as usize);
                    Value::LoroValue(value.into())
                }
                crate::container::list::list_op::InnerListOp::InsertText { slice, .. } => {
                    Value::Str(std::str::from_utf8(slice.as_bytes()).unwrap())
                }
                crate::container::list::list_op::InnerListOp::Delete(span) => {
                    delete_start.push(EncodedDeleteStartId {
                        peer_idx: registers.peer_mut().register(&span.id_start.peer),
                        counter: span.id_start.counter,
                        len: span.span.signed_len,
                    });
                    Value::DeleteSeq
                }
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => Value::MarkStart(MarkStart {
                    len: *end - *start,
                    key: key.clone(),
                    value: value.clone(),
                    info: info.to_byte(),
                }),
                crate::container::list::list_op::InnerListOp::Set { elem_id, value } => {
                    Value::ListSet {
                        peer_idx: registers.peer_mut().register(&elem_id.peer),
                        lamport: elem_id.lamport,
                        value: value.clone(),
                    }
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => Value::Null,
                crate::container::list::list_op::InnerListOp::Move {
                    from,
                    elem_id: from_id,
                    to: _,
                } => Value::ListMove {
                    from: *from as usize,
                    from_idx: registers.peer_mut().register(&from_id.peer),
                    lamport: from_id.lamport as usize,
                },
            },
            crate::op::InnerContent::Map(map) => {
                assert_eq!(op.container.get_type(), ContainerType::Map);
                match &map.value {
                    Some(v) => Value::LoroValue(v.clone()),
                    None => Value::DeleteOnce,
                }
            }
            crate::op::InnerContent::Tree(t) => {
                assert_eq!(op.container.get_type(), ContainerType::Tree);
                registers.encode_tree_op(t)
            }
            crate::op::InnerContent::Future(f) => match f {
                #[cfg(feature = "counter")]
                FutureInnerContent::Counter(c) => {
                    let c_abs = c.abs();
                    if c_abs.fract() < f64::EPSILON && (c_abs as i64) < (2 << 26) {
                        Value::I64(*c as i64)
                    } else {
                        Value::F64(*c)
                    }
                }
                FutureInnerContent::Unknown { value, .. } => Value::from_owned(value),
            },
        };
        let (k, _) = value.encode(value_writer, registers);
        k
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_op(
    cid: &ContainerID,
    value: Value<'_>,
    del_iter: &mut impl Iterator<Item = Result<EncodedDeleteStartId, ColumnarError>>,
    shared_arena: &SharedArena,
    arenas: &dyn ValueDecodedArenasTrait,
    positions: &[Vec<u8>],
    prop: i32,
    op_id: ID,
) -> LoroResult<crate::op::InnerContent> {
    let content = match cid.container_type() {
        ContainerType::Text => match value {
            Value::Str(s) => {
                let (slice, result) = shared_arena.alloc_str_with_slice(s);
                crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::InsertText {
                        slice,
                        unicode_start: result.start as u32,
                        unicode_len: (result.end - result.start) as u32,
                        pos: prop as u32,
                    },
                )
            }
            Value::DeleteSeq => {
                let del_start = del_iter.next().unwrap()?;
                let peer_idx = del_start.peer_idx;
                let cnt = del_start.counter;
                let len = del_start.len;
                crate::op::InnerContent::List(crate::container::list::list_op::InnerListOp::Delete(
                    DeleteSpanWithId::new(
                        ID::new(arenas.peers()[peer_idx], cnt as Counter),
                        prop as isize,
                        len,
                    ),
                ))
            }
            Value::MarkStart(mark) => crate::op::InnerContent::List(
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start: prop as u32,
                    end: prop as u32 + mark.len,
                    key: mark.key,
                    value: mark.value,
                    info: TextStyleInfoFlag::from_byte(mark.info),
                },
            ),
            Value::Null => crate::op::InnerContent::List(
                crate::container::list::list_op::InnerListOp::StyleEnd,
            ),
            _ => unreachable!(),
        },
        ContainerType::Map => {
            let key = arenas
                .keys()
                .get(prop as usize)
                .ok_or(LoroError::DecodeDataCorruptionError)?
                .clone();
            match value {
                Value::DeleteOnce => {
                    crate::op::InnerContent::Map(crate::container::map::MapSet { key, value: None })
                }
                Value::LoroValue(v) => {
                    crate::op::InnerContent::Map(crate::container::map::MapSet {
                        key,
                        value: Some(v.clone()),
                    })
                }
                _ => unreachable!(),
            }
        }
        ContainerType::List => {
            let pos = prop as usize;
            match value {
                Value::LoroValue(arr) => {
                    let range = shared_arena.alloc_values(arr.into_list().unwrap().iter().cloned());
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Insert {
                            slice: SliceRange::new(range.start as u32..range.end as u32),
                            pos,
                        },
                    )
                }
                Value::DeleteSeq => {
                    let del_start = del_iter.next().unwrap()?;
                    let peer_idx = del_start.peer_idx;
                    let cnt = del_start.counter;
                    let len = del_start.len;
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Delete(
                            DeleteSpanWithId::new(
                                ID::new(arenas.peers()[peer_idx], cnt as Counter),
                                pos as isize,
                                len,
                            ),
                        ),
                    )
                }
                _ => unreachable!(),
            }
        }
        ContainerType::Tree => match value {
            Value::TreeMove(op) => crate::op::InnerContent::Tree(Arc::new(
                arenas.decode_tree_op(positions, op, op_id)?,
            )),
            Value::RawTreeMove(op) => {
                let subject = TreeID::new(
                    arenas.peers()[op.subject_peer_idx],
                    op.subject_cnt as Counter,
                );
                let parent = if op.is_parent_null {
                    None
                } else {
                    let parent_id =
                        TreeID::new(arenas.peers()[op.parent_peer_idx], op.parent_cnt as Counter);
                    if parent_id.is_deleted_root() {
                        return Ok(crate::op::InnerContent::Tree(Arc::new(TreeOp::Delete {
                            target: subject,
                        })));
                    }

                    Some(parent_id)
                };

                let fi = FractionalIndex::from_bytes(positions[op.position_idx].clone());
                let is_create = subject.id() == op_id;
                let ans = if is_create {
                    TreeOp::Create {
                        target: subject,
                        parent,
                        position: fi,
                    }
                } else {
                    TreeOp::Move {
                        target: subject,
                        parent,
                        position: fi,
                    }
                };
                crate::op::InnerContent::Tree(Arc::new(ans))
            }
            _ => {
                unreachable!()
            }
        },
        ContainerType::MovableList => {
            let pos = prop as usize;
            match value {
                Value::LoroValue(arr) => {
                    let range = shared_arena.alloc_values(arr.into_list().unwrap().iter().cloned());
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Insert {
                            slice: SliceRange::new(range.start as u32..range.end as u32),
                            pos,
                        },
                    )
                }
                Value::DeleteSeq => {
                    let del_start = del_iter.next().unwrap()?;
                    let peer_idx = del_start.peer_idx;
                    let cnt = del_start.counter;
                    let len = del_start.len;
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Delete(
                            DeleteSpanWithId::new(
                                ID::new(arenas.peers()[peer_idx], cnt as Counter),
                                pos as isize,
                                len,
                            ),
                        ),
                    )
                }
                Value::ListMove {
                    from,
                    from_idx,
                    lamport,
                } => crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::Move {
                        from: from as u32,
                        elem_id: IdLp::new(arenas.peers()[from_idx], lamport as Lamport),
                        to: prop as u32,
                    },
                ),
                Value::ListSet {
                    peer_idx,
                    lamport,
                    value,
                } => crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::Set {
                        elem_id: IdLp::new(arenas.peers()[peer_idx], lamport as Lamport),
                        value,
                    },
                ),
                _ => unreachable!(),
            }
        }
        #[cfg(feature = "counter")]
        ContainerType::Counter => match value {
            Value::F64(c) => crate::op::InnerContent::Future(FutureInnerContent::Counter(c)),
            Value::I64(c) => crate::op::InnerContent::Future(FutureInnerContent::Counter(c as f64)),
            _ => unreachable!(),
        },
        // NOTE: The future container type need also try to parse the unknown type
        ContainerType::Unknown(_) => crate::op::InnerContent::Future(FutureInnerContent::Unknown {
            prop,
            value: Box::new(value.into_owned()),
        }),
    };

    Ok(content)
}

pub type PeerIdx = usize;

#[columnar(ser, de)]
struct EncodedDoc<'a> {
    #[columnar(class = "vec", iter = "EncodedOp")]
    ops: Vec<EncodedOp>,
    #[columnar(class = "vec", iter = "EncodedChange")]
    changes: Vec<EncodedChange>,
    #[columnar(class = "vec", iter = "EncodedDeleteStartId")]
    delete_starts: Vec<EncodedDeleteStartId>,
    /// Container states snapshot.
    ///
    /// It's empty when the encoding mode is not snapshot.
    #[columnar(class = "vec", iter = "EncodedStateInfo")]
    states: Vec<EncodedStateInfo>,
    /// The first counter value for each change of each peer in `changes`
    start_counters: Vec<Counter>,
    /// The frontiers at the start of this encoded delta.
    ///
    /// It's empty when the encoding mode is snapshot.
    start_frontiers: Vec<(PeerIdx, Counter)>,
    #[columnar(borrow)]
    raw_values: Cow<'a, [u8]>,

    /// A list of encoded arenas, in the following order
    /// - `peer_id_arena`
    /// - `container_arena`
    /// - `key_arena`
    /// - `deps_arena`
    /// - `state_arena`
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
    #[columnar(strategy = "DeltaRle")]
    peer_idx: u32,
    #[columnar(strategy = "DeltaRle")]
    value_type: u8,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
pub(crate) struct EncodedDeleteStartId {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
    #[columnar(strategy = "DeltaRle")]
    len: isize,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedChange {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    len: usize,
    #[columnar(strategy = "DeltaRle")]
    timestamp: i64,
    #[columnar(strategy = "DeltaRle")]
    deps_len: i32,
    #[columnar(strategy = "BoolRle")]
    dep_on_self: bool,
    #[columnar(strategy = "DeltaRle")]
    msg_idx_plus_one: i32,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedStateInfo {
    #[columnar(strategy = "DeltaRle")]
    container_index: u32,
    #[columnar(strategy = "DeltaRle")]
    op_len: u32,
    #[columnar(strategy = "DeltaRle")]
    state_bytes_len: u32,
    #[columnar(strategy = "BoolRle")]
    is_unknown: bool,
}

#[cfg(test)]
mod test {

    use loro_common::LoroValue;

    use crate::{encoding::value_register::ValueRegister, fx_map};

    use super::*;

    fn test_loro_value_read_write(v: impl Into<LoroValue>, container_id: Option<ContainerID>) {
        let v = v.into();
        let id = match &container_id {
            Some(ContainerID::Root { .. }) => ID::new(u64::MAX, 0),
            Some(ContainerID::Normal { peer, counter, .. }) => ID::new(*peer, *counter),
            None => ID::new(u64::MAX, 0),
        };

        let mut registers = EncodedRegisters {
            key: ValueRegister::new(),
            container: ValueRegister::new(),
            peer: ValueRegister::new(),
            tree_id: ValueRegister::new(),
            position: either::Either::Left(FxHashSet::default()),
        };

        let mut writer = ValueWriter::new();
        let (kind, _) = writer.write_value_content(&v, &mut registers);

        let binding = writer.finish();
        let mut reader = ValueReader::new(binding.as_slice());

        let ans = reader
            .read_value_content(kind, &registers.key.unwrap_vec(), id)
            .unwrap();
        assert_eq!(v, ans)
    }

    #[test]
    fn test_value_read_write() {
        test_loro_value_read_write(true, None);
        test_loro_value_read_write(false, None);
        test_loro_value_read_write(123, None);
        test_loro_value_read_write(1.23, None);
        test_loro_value_read_write(LoroValue::Null, None);
        test_loro_value_read_write(
            LoroValue::Binary((vec![123, 223, 255, 0, 1, 2, 3]).into()),
            None,
        );
        test_loro_value_read_write("sldk;ajfas;dlkfas测试", None);
        // we won't encode root container by `value content`
        // test_loro_value_read_write(
        //     LoroValue::Container(ContainerID::new_root("name", ContainerType::Text)),
        //     Some(ContainerID::new_root("name", ContainerType::Text)),
        // );
        test_loro_value_read_write(
            LoroValue::Container(ContainerID::new_normal(
                ID::new(u64::MAX, 123),
                ContainerType::Tree,
            )),
            Some(ContainerID::new_normal(
                ID::new(u64::MAX, 123),
                ContainerType::Tree,
            )),
        );
        test_loro_value_read_write(vec![1i32, 2, 3], None);
        test_loro_value_read_write(
            LoroValue::Map(
                (fx_map![
                    "1".into() => 123.into(),
                    "2".into() => "123".into(),
                    "3".into() => vec![true].into()
                ])
                .into(),
            ),
            None,
        );
    }
}
