use std::{
    mem,
    sync::{Arc, Mutex, Weak},
};

use fxhash::FxHashMap;
use loro_common::{ContainerID, IdLp, LoroResult};
use rle::HasLength;

use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, map::MapSet},
    delta::{MapValue, ResolvedMapDelta, ResolvedMapValue},
    encoding::{EncodeMode, StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    handler::ValueOrHandler,
    op::{Op, RawOp, RawOpContent},
    txn::Transaction,
    DocState, InternalString, LoroValue,
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct MapState {
    idx: ContainerIdx,
    map: FxHashMap<InternalString, MapValue>,
    size: usize,
}

impl ContainerState for MapState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        self.map.capacity() * (mem::size_of::<MapValue>() + mem::size_of::<InternalString>())
    }

    fn is_state_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let InternalDiff::Map(delta) = diff else {
            unreachable!()
        };
        let mut resolved_delta = ResolvedMapDelta::new();
        for (key, value) in delta.updated.into_iter() {
            self.insert(key.clone(), value.clone());
            resolved_delta = resolved_delta.with_entry(
                key,
                ResolvedMapValue {
                    idlp: IdLp::new(value.peer, value.lamp),
                    value: value
                        .value
                        .map(|v| ValueOrHandler::from_value(v, arena, txn, state)),
                },
            )
        }

        Diff::Map(resolved_delta)
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) {
        let _ = self.apply_diff_and_convert(diff, arena, txn, state);
    }

    fn apply_local_op(&mut self, op: &RawOp, _: &Op) -> LoroResult<()> {
        match &op.content {
            RawOpContent::Map(MapSet { key, value }) => {
                if value.is_none() {
                    self.insert(
                        key.clone(),
                        MapValue {
                            lamp: op.lamport,
                            peer: op.id.peer,
                            value: None,
                        },
                    );
                    return Ok(());
                }

                self.insert(
                    key.clone(),
                    MapValue {
                        lamp: op.lamport,
                        peer: op.id.peer,
                        value: Some(value.clone().unwrap()),
                    },
                );
                Ok(())
            }
            _ => unreachable!(),
        }
    }

    #[doc = " Convert a state to a diff that when apply this diff on a empty state,"]
    #[doc = " the state will be the same as this state."]
    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        Diff::Map(ResolvedMapDelta {
            updated: self
                .map
                .clone()
                .into_iter()
                .map(|(k, v)| (k, ResolvedMapValue::from_map_value(v, arena, txn, state)))
                .collect::<FxHashMap<_, _>>(),
        })
    }

    fn get_value(&mut self) -> LoroValue {
        let ans = self.to_map();
        LoroValue::Map(Arc::new(ans))
    }

    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        for (key, value) in self.map.iter() {
            if let Some(LoroValue::Container(x)) = &value.value {
                if x == id {
                    return Some(Index::Key(key.clone()));
                }
            }
        }

        None
    }

    fn contains_child(&self, id: &ContainerID) -> bool {
        for (_, value) in self.map.iter() {
            if let Some(LoroValue::Container(x)) = &value.value {
                if x == id {
                    return true;
                }
            }
        }

        false
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        let mut ans = Vec::new();
        for (_, value) in self.map.iter() {
            if let Some(LoroValue::Container(x)) = &value.value {
                ans.push(x.clone());
            }
        }
        ans
    }

    #[doc = " Get a list of ops that can be used to restore the state to the current state"]
    fn encode_snapshot(&self, mut encoder: StateSnapshotEncoder) -> Vec<u8> {
        for v in self.map.values() {
            encoder.encode_op(v.idlp().into(), || unimplemented!());
        }

        Default::default()
    }

    #[doc = " Restore the state to the state represented by the ops that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) -> LoroResult<()> {
        assert_eq!(ctx.mode, EncodeMode::Snapshot);
        for op in ctx.ops {
            debug_assert_eq!(
                op.op.atom_len(),
                1,
                "MapState::from_snapshot_ops: op.atom_len() != 1"
            );

            let content = op.op.content.as_map().unwrap();
            self.insert(
                content.key.clone(),
                MapValue {
                    value: content.value.clone(),
                    lamp: op.lamport.expect("op should already be imported"),
                    peer: op.peer,
                },
            );
        }
        Ok(())
    }
}

impl MapState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            map: FxHashMap::default(),
            size: 0,
        }
    }

    pub fn insert(&mut self, key: InternalString, value: MapValue) {
        let value_yes = value.value.is_some();
        let result = self.map.insert(key.clone(), value);
        match (result, value_yes) {
            (Some(x), true) => {
                if let None = x.value {
                    self.size += 1;
                }
            }
            (None, true) => {
                self.size += 1;
            }
            (Some(x), false) => {
                if let Some(_) = x.value {
                    self.size -= 1;
                }
            }
            _ => {}
        };
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, InternalString, MapValue> {
        self.map.iter()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    fn to_map(
        &self,
    ) -> std::collections::HashMap<String, LoroValue, std::hash::BuildHasherDefault<fxhash::FxHasher>>
    {
        let mut ans = FxHashMap::with_capacity_and_hasher(self.map.len(), Default::default());
        for (key, value) in self.map.iter() {
            if value.value.is_none() {
                continue;
            }

            ans.insert(key.to_string(), value.value.as_ref().cloned().unwrap());
        }
        ans
    }

    pub fn get(&self, k: &str) -> Option<&LoroValue> {
        match self.map.get(&k.into()) {
            Some(value) => match &value.value {
                Some(v) => Some(v),
                None => None,
            },
            None => None,
        }
    }
}
