use crate::change::Change;
use crate::id::{Counter, PeerID, ID};
use crate::op::RemoteOp;
use crate::span::{CounterSpan, HasCounter, HasCounterSpan};

use crate::{container::registry::ContainerIdx, event::Diff, version::Frontiers};
use itertools::Itertools;
use loro_common::IdSpanVector;
use smallvec::{smallvec, SmallVec};
use std::{collections::VecDeque, sync::MutexGuard};
use tracing::instrument;

use fxhash::{FxHashMap, FxHashSet};

use rle::{slice_vec_by, HasLength, Sliceable};

use crate::{
    container::ContainerID,
    dag::{remove_included_frontiers, DagUtils},
    op::RichOp,
    span::{HasIdSpan, HasLamportSpan, IdSpan},
    version::are_frontiers_eq,
    VersionVector,
};

use super::RemoteClientChanges;

#[derive(Debug)]
pub struct ImportContext {
    // pub old_frontiers: Frontiers,
    pub new_frontiers: Frontiers,
    pub old_vv: VersionVector,
    pub new_vv: VersionVector,
    pub spans: IdSpanVector,
    pub diff: Vec<(ContainerID, SmallVec<[Diff; 1]>)>,
}

impl ImportContext {
    pub fn push_diff(&mut self, id: &ContainerID, diff: Diff) {
        if let Some((last_id, vec)) = self.diff.last_mut() {
            if last_id == id {
                vec.push(diff);
                return;
            }
        }

        self.diff.push((id.clone(), smallvec![diff]));
    }

    pub fn push_diff_vec(&mut self, id: &ContainerID, mut diff: SmallVec<[Diff; 1]>) {
        if let Some((last_id, vec)) = self.diff.last_mut() {
            if last_id == id {
                vec.append(&mut diff);
                return;
            }
        }

        self.diff.push((id.clone(), diff));
    }
}

#[derive(Debug)]
enum ChangeApplyState {
    Existing,
    Directly,
    /// The client id of first missing dep
    Future(PeerID),
}

fn can_remote_change_be_applied(
    vv: &VersionVector,
    change: &mut Change<RemoteOp>,
) -> ChangeApplyState {
    let change_client_id = change.id.peer;
    let CounterSpan { start, end } = change.ctr_span();
    let vv_latest_ctr = vv.get(&change_client_id).copied().unwrap_or(0);
    if vv_latest_ctr < start {
        return ChangeApplyState::Future(change_client_id);
    }
    if vv_latest_ctr >= end || start == end {
        return ChangeApplyState::Existing;
    }
    for dep in change.deps.iter() {
        let dep_vv_latest_ctr = vv.get(&dep.peer).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeApplyState::Future(dep.peer);
        }
    }

    if start < vv_latest_ctr {
        *change = change.slice((vv_latest_ctr - start) as usize, (end - start) as usize);
    }

    ChangeApplyState::Directly
}
