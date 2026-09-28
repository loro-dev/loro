//! `MovableListHandler::apply_delta`: turn a list delta into local movable-list
//! ops.
//!
//! A delete of a child container followed by an insert of the same container
//! id is applied as a move, so the child keeps its identity and content. The
//! delta is applied in three passes:
//!
//! 1. Plan: record the delete ranges in original indices and decide which
//!    deleted children are re-inserted ("claimed").
//! 2. Delete every unclaimed element of the delete ranges, right to left, so
//!    the remaining original indices stay valid.
//! 3. Walk the delta again with a cursor in live indices. Every inserted value
//!    is placed right after the previously placed target element: new values
//!    and containers are inserted there, claimed children are moved there.
//!
//! In pass 3, `live[cursor..]` is always the unplaced claimed children of the
//! current delete gap followed by the unmoved original elements at or after
//! the delta position. A retain therefore skips those pending children plus
//! the retained run. A claimed child found before the cursor was left behind by
//! an earlier retain; moving it to `cursor - 1` keeps the cursor in place.

use super::*;
use crate::{event::Index, state::ContainerState};
use std::ops::Range;

/// Pass 1 output of [`MovableListHandler::apply_delta`].
struct MovePlan {
    /// Delete ranges of the delta in original indices, ascending.
    deleted: Vec<Range<usize>>,
    /// Original indices of the deleted children that the delta re-inserts,
    /// ascending.
    claimed: Vec<usize>,
    /// One entry per inserted container value, in delta order: the original
    /// index and id of the deleted child it claims, if any.
    claims: Vec<Option<(usize, ContainerID)>>,
}

impl MovableListHandler {
    /// Applies a list delta to the movable list.
    ///
    /// Container values inserted by the delta move the matching child out of a
    /// deleted range of the same delta when there is one, following
    /// `container_remap` for children recreated by earlier undo/apply_diff
    /// calls. Other container values create new children and record the
    /// `old id -> new id` mapping in `container_remap`. An insert with
    /// `from_move` whose child still exists elsewhere in the list is skipped.
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn apply_delta(
        &self,
        delta: loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
    ) -> LoroResult<()> {
        {
            // Test whether the delta is valid
            let len = self.len();
            let mut index = 0;
            for delta_item in delta.iter() {
                match delta_item {
                    loro_delta::DeltaItem::Retain { len, .. } => {
                        index += *len;
                    }
                    loro_delta::DeltaItem::Replace { delete, .. } => {
                        index += *delete;
                    }
                }

                if index > len {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        len,
                        info: "apply_delta".into(),
                    });
                }
            }
        }

        if let MaybeDetached::Detached(_) = &self.inner {
            unimplemented!();
        }

        let plan = self.plan_moves(&delta, container_remap)?;
        self.delete_unclaimed(&plan)?;
        self.place_inserted_values(&delta, container_remap, plan)
    }

    fn plan_moves(
        &self,
        delta: &loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
        container_remap: &FxHashMap<ContainerID, ContainerID>,
    ) -> LoroResult<MovePlan> {
        let mut deleted = Vec::new();
        let mut deleted_children: FxHashMap<ContainerID, usize> = FxHashMap::default();
        self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            let mut index = 0;
            for delta_item in delta.iter() {
                match delta_item {
                    loro_delta::DeltaItem::Retain { len, .. } => {
                        index += *len;
                    }
                    loro_delta::DeltaItem::Replace { delete, .. } => {
                        if *delete == 0 {
                            continue;
                        }

                        for i in index..index + *delete {
                            if let Some(LoroValue::Container(c)) = list.get(i, IndexType::ForUser) {
                                deleted_children.insert(c.clone(), i);
                            }
                        }
                        deleted.push(index..index + *delete);
                        index += *delete;
                    }
                }
            }
            Ok(())
        })?;

        let mut claimed = Vec::new();
        let mut claims = Vec::new();
        if !deleted_children.is_empty() {
            for delta_item in delta.iter() {
                let loro_delta::DeltaItem::Replace { value, .. } = delta_item else {
                    continue;
                };
                for v in value.iter() {
                    let Some(mut id) = inserted_container_id(v) else {
                        continue;
                    };
                    if !deleted_children.contains_key(&id) {
                        while let Some(new_id) = container_remap.get(&id) {
                            id = new_id.clone();
                            if deleted_children.contains_key(&id) {
                                break;
                            }
                        }
                    }
                    let claim = deleted_children.remove(&id).map(|i| (i, id));
                    if let Some((i, _)) = &claim {
                        claimed.push(*i);
                    }
                    claims.push(claim);
                }
            }
            claimed.sort_unstable();
        }

        Ok(MovePlan {
            deleted,
            claimed,
            claims,
        })
    }

    fn delete_unclaimed(&self, plan: &MovePlan) -> LoroResult<()> {
        let mut claimed = plan.claimed.iter().rev().peekable();
        for range in plan.deleted.iter().rev() {
            let mut end = range.end;
            while let Some(&&i) = claimed.peek() {
                if i < range.start {
                    break;
                }
                debug_assert!(i < range.end);
                claimed.next();
                if end > i + 1 {
                    self.delete(i + 1, end - i - 1)?;
                }
                end = i;
            }
            if end > range.start {
                self.delete(range.start, end - range.start)?;
            }
        }

        Ok(())
    }

    fn place_inserted_values(
        &self,
        delta: &loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        plan: MovePlan,
    ) -> LoroResult<()> {
        let MovePlan {
            claimed, claims, ..
        } = plan;
        let mut claims = claims.into_iter();
        let mut placed = vec![false; claimed.len()];
        // Live index where the next target element goes
        let mut cursor = 0;
        // Delta position in original indices
        let mut orig = 0;
        // Start of the deletions since the last retain, in original indices
        let mut gap_start = 0;
        // Unplaced claimed children of the current gap; they are at
        // `live[cursor..cursor + pending]`
        let mut pending = 0;
        for delta_item in delta.iter() {
            match delta_item {
                loro_delta::DeltaItem::Retain { len, .. } => {
                    cursor += pending + *len;
                    pending = 0;
                    orig += *len;
                    gap_start = orig;
                }
                loro_delta::DeltaItem::Replace {
                    value,
                    delete,
                    attr,
                } => {
                    if *delete > 0 {
                        let start = claimed.partition_point(|&i| i < orig);
                        let end = claimed.partition_point(|&i| i < orig + *delete);
                        pending += placed[start..end].iter().filter(|&&p| !p).count();
                        orig += *delete;
                    }

                    for v in value.iter() {
                        let id = match v {
                            ValueOrHandler::Value(LoroValue::Container(id)) => id.clone(),
                            ValueOrHandler::Handler(h) => h.id(),
                            ValueOrHandler::Value(v) => {
                                self.insert(cursor, v.clone())?;
                                cursor += 1;
                                continue;
                            }
                        };

                        match claims.next().flatten() {
                            Some((orig_index, child)) => {
                                let k = claimed.binary_search(&orig_index).unwrap();
                                placed[k] = true;
                                if gap_start <= orig_index && orig_index < orig {
                                    pending -= 1;
                                }
                                let from = self.child_index(&child)?;
                                if from >= cursor {
                                    self.mov(from, cursor)?;
                                    cursor += 1;
                                } else {
                                    self.mov(from, cursor - 1)?;
                                }
                            }
                            None => {
                                let mut id = id;
                                while let Some(new_id) = container_remap.get(&id) {
                                    id = new_id.clone();
                                }
                                if !attr.from_move || !self.contains_child(&id)? {
                                    let new_handler = self.insert_container(
                                        cursor,
                                        Handler::new_unattached(id.container_type()),
                                    )?;
                                    container_remap.insert(id, new_handler.id());
                                    cursor += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        debug_assert_eq!(pending, 0);
        debug_assert!(placed.iter().all(|&p| p));
        Ok(())
    }

    /// Live index of a child container that must be in the list.
    fn child_index(&self, id: &ContainerID) -> LoroResult<usize> {
        let index = self.with_state(|state| Ok(state.get_child_index(id)))?;
        match index {
            Some(Index::Seq(i)) => Ok(i),
            _ => panic!(
                "moved child {id} is missing from movable list {}",
                self.id()
            ),
        }
    }

    fn contains_child(&self, id: &ContainerID) -> LoroResult<bool> {
        self.with_state(|state| Ok(state.contains_child(id)))
    }
}

fn inserted_container_id(v: &ValueOrHandler) -> Option<ContainerID> {
    match v {
        ValueOrHandler::Value(LoroValue::Container(id)) => Some(id.clone()),
        ValueOrHandler::Handler(h) => Some(h.id()),
        ValueOrHandler::Value(_) => None,
    }
}
