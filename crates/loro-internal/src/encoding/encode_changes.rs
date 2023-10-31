use std::{collections::VecDeque, ops::Range, sync::Arc};

use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::TreeID;
use rle::{HasLength, RleVec};
use serde_columnar::{columnar, iter_from_bytes, to_vec};

use crate::{
    change::Lamport,
    container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        tree::tree_op::TreeOp,
        ContainerID, ContainerType,
    },
    encoding::RemoteClientChanges,
    id::{Counter, PeerID, ID},
    op::{ListSlice, RawOpContent, RemoteOp},
    oplog::OpLog,
    span::HasId,
    version::Frontiers,
};

type ClientIdx = u32;
type Clients = Vec<PeerID>;
type Containers = Vec<ContainerID>;

pub(crate) fn get_lamport_by_deps_oplog(
    deps: &Frontiers,
    lamport_map: &FxHashMap<PeerID, Vec<(Range<Counter>, Lamport)>>,
    oplog: Option<&OpLog>,
) -> Result<Lamport, PeerID> {
    let mut ans = 0;
    for id in deps.iter() {
        if let Some(c) = oplog.and_then(|x| x.lookup_change(*id)) {
            let offset = id.counter - c.id.counter;
            ans = ans.max(c.lamport + offset as u32 + 1);
        } else if let Some(v) = lamport_map.get(&id.peer) {
            if let Some((lamport, offset)) = get_value_from_range_map(v, id.counter) {
                ans = ans.max(lamport + offset + 1);
            } else {
                return Err(id.peer);
            }
        } else {
            return Err(id.peer);
        }
    }
    Ok(ans)
}

fn get_value_from_range_map(
    v: &[(Range<Counter>, Lamport)],
    key: Counter,
) -> Option<(Lamport, u32)> {
    let index = match v.binary_search_by_key(&key, |(range, _)| range.start) {
        Ok(index) => Some(index),

        // If the requested key is smaller than the smallest range in the slice,
        // we would be computing `0 - 1`, which would underflow an `usize`.
        // We use `checked_sub` to get `None` instead.
        Err(index) => index.checked_sub(1),
    };

    if let Some(index) = index {
        let (ref range, value) = v[index];
        if key < range.end {
            return Some((value, (key - range.start) as u32));
        }
    }
    None
}
