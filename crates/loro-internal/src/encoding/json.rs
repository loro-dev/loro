use std::borrow::Cow;

use json_schema::{JsonLoroValue, LoroJsonSchema, OpContent};
use loro_common::{ContainerType, TreeID};
use rle::{HasLength, Sliceable};

use crate::{
    arena::SharedArena,
    change::Change,
    container::{
        list::list_op::{DeleteSpan, DeleteSpanWithId, InnerListOp},
        map::MapSet,
        tree::tree_op::TreeOp,
    },
    op::{InnerContent, Op},
    LoroDoc, OpLog, VersionVector,
};

pub(crate) fn export_json(oplog: &OpLog, vv: &VersionVector) -> LoroJsonSchema {
    let actual_start_vv: VersionVector = vv
        .iter()
        .filter_map(|(&peer, &end_counter)| {
            if end_counter == 0 {
                return None;
            }

            let this_end = oplog.vv().get(&peer).cloned().unwrap_or(0);
            if this_end <= end_counter {
                return Some((peer, this_end));
            }

            Some((peer, end_counter))
        })
        .collect();
    let diff_changes = init_encode(oplog, &actual_start_vv);
    let changes = encode_changes(&diff_changes, &oplog.arena);
    LoroJsonSchema {
        changes,
        loro_version: "0.1.0".to_string(),
        // TODO:
        start_vv: format!("{:?}", actual_start_vv),
        end_vv: format!("{:?}", oplog.vv()),
    };
    todo!()
}

fn init_encode<'a>(oplog: &'a OpLog, vv: &'_ VersionVector) -> Vec<Cow<'a, Change>> {
    let self_vv = oplog.vv();
    let start_vv = vv.trim(oplog.vv());
    let mut diff_changes: Vec<Cow<'a, Change>> = Vec::new();
    for change in oplog.iter_changes_peer_by_peer(&start_vv, self_vv) {
        let start_cnt = start_vv.get(&change.id.peer).copied().unwrap_or(0);
        if change.id.counter < start_cnt {
            let offset = start_cnt - change.id.counter;
            diff_changes.push(Cow::Owned(change.slice(offset as usize, change.atom_len())));
        } else {
            diff_changes.push(Cow::Borrowed(change));
        }
    }
    diff_changes.sort_by_key(|x| x.lamport);
    diff_changes
}

fn encode_changes(
    diff_changes: &[Cow<'_, Change>],
    arena: &SharedArena,
) -> Vec<json_schema::Change> {
    let mut changes = Vec::with_capacity(diff_changes.len());
    for change in diff_changes.iter() {
        let mut ops = Vec::with_capacity(change.ops().len());
        for Op {
            counter,
            container,
            content,
        } in change.ops().iter()
        {
            let container = arena.get_container_id(*container).unwrap();
            let op = match container.container_type() {
                ContainerType::List => match content {
                    InnerContent::List(list) => OpContent::List(match list {
                        InnerListOp::Insert { slice, pos } => {
                            let value =
                                arena.get_values(slice.0.start as usize..slice.0.end as usize);
                            json_schema::ListOp::Insert {
                                pos: *pos,
                                value: JsonLoroValue(value.into()),
                            }
                        }
                        InnerListOp::Delete(DeleteSpanWithId {
                            id_start,
                            span: DeleteSpan { pos, signed_len },
                        }) => json_schema::ListOp::Delete {
                            pos: *pos as usize,
                            len: *signed_len,
                            delete_start_id: *id_start,
                        },
                        _ => unreachable!(),
                    }),
                    _ => unreachable!(),
                },
                ContainerType::MovableList => match content {
                    InnerContent::List(list) => OpContent::MovableList(match list {
                        InnerListOp::Insert { slice, pos } => {
                            let value =
                                arena.get_values(slice.0.start as usize..slice.0.end as usize);
                            json_schema::MovableListOp::Insert {
                                pos: *pos,
                                value: JsonLoroValue(value.into()),
                            }
                        }
                        InnerListOp::Delete(DeleteSpanWithId {
                            id_start,
                            span: DeleteSpan { pos, signed_len },
                        }) => json_schema::MovableListOp::Delete {
                            pos: *pos as usize,
                            len: *signed_len,
                            delete_start_id: *id_start,
                        },
                        InnerListOp::Move { from, from_id, to } => {
                            json_schema::MovableListOp::Move {
                                from: *from,
                                to: *to,
                                from_id: *from_id,
                            }
                        }
                        InnerListOp::Set { elem_id, value } => json_schema::MovableListOp::Set {
                            elem_id: *elem_id,
                            value: JsonLoroValue(value.clone()),
                        },
                        _ => unreachable!(),
                    }),
                    _ => unreachable!(),
                },
                ContainerType::Text => match content {
                    InnerContent::List(list) => OpContent::Text(match list {
                        InnerListOp::InsertText {
                            slice,
                            unicode_start,
                            unicode_len,
                            pos,
                        } => {
                            let text = String::from_utf8(slice.as_bytes().to_vec()).unwrap();
                            json_schema::TextOp::Insert {
                                pos: *pos as usize,
                                text,
                            }
                        }
                        InnerListOp::Delete(DeleteSpanWithId {
                            id_start,
                            span: DeleteSpan { pos, signed_len },
                        }) => json_schema::TextOp::Delete {
                            pos: *pos as usize,
                            len: *signed_len as usize,
                            // delete_start_id: *id_start,
                        },
                        InnerListOp::StyleStart {
                            start,
                            end,
                            key,
                            value,
                            info,
                        } => json_schema::TextOp::Mark {
                            start: *start,
                            end: *end,
                            style: (key.to_string(), JsonLoroValue(value.clone())),
                        },
                        InnerListOp::StyleEnd => json_schema::TextOp::MarkEnd,
                        _ => unreachable!(),
                    }),
                    _ => unreachable!(),
                },
                ContainerType::Map => match content {
                    InnerContent::Map(MapSet { key, value }) => {
                        OpContent::Map(if let Some(v) = value {
                            json_schema::MapOp::Insert {
                                key: key.to_string(),
                                value: JsonLoroValue(v.clone()),
                            }
                        } else {
                            json_schema::MapOp::Delete {
                                key: key.to_string(),
                            }
                        })
                    }

                    _ => unreachable!(),
                },

                ContainerType::Tree => match content {
                    // TODO: how to determine the type of the tree op?
                    InnerContent::Tree(TreeOp {
                        target,
                        parent,
                        position,
                    }) => OpContent::Tree({
                        if let Some(p) = parent {
                            if TreeID::is_deleted_root(p) {
                                json_schema::TreeOp::Delete { target: *target }
                            } else {
                                json_schema::TreeOp::Move {
                                    target: *target,
                                    parent: *parent,
                                    fractional_index: position.as_ref().unwrap().to_string(),
                                }
                            }
                        } else {
                            json_schema::TreeOp::Move {
                                target: *target,
                                parent: None,
                                fractional_index: position.as_ref().unwrap().to_string(),
                            }
                        }
                    }),
                    _ => unreachable!(),
                },
                ContainerType::Unknown(u) => {
                    // TODO:
                    todo!()
                }
            };
            ops.push(json_schema::Op {
                counter: *counter,
                container,
                content: op,
            });
        }
        let c = json_schema::Change {
            id: change.id,
            ops,
            deps: change.deps.iter().copied().collect(),
            lamport: change.lamport,
            timestamp: change.timestamp,
            msg: None,
        };
        changes.push(c);
    }
    changes
}
