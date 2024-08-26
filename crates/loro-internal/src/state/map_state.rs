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
    diff_calc::DiffMode,
    encoding::{EncodeMode, StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    handler::ValueOrHandler,
    op::{Op, RawOp, RawOpContent},
    txn::Transaction,
    DocState, InternalString, LoroValue,
};

use super::{ContainerState, DiffApplyContext};

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
        DiffApplyContext {
            arena,
            txn,
            state,
            mode,
        }: DiffApplyContext,
    ) -> Diff {
        let InternalDiff::Map(delta) = diff else {
            unreachable!()
        };
        let force = matches!(mode, DiffMode::Checkout | DiffMode::Linear);
        let mut resolved_delta = ResolvedMapDelta::new();
        for (key, value) in delta.updated.into_iter() {
            let Some(value) = value else {
                // uncreate op
                assert_eq!(mode, DiffMode::Checkout);
                self.remove(&key);
                resolved_delta = resolved_delta.with_entry(key, ResolvedMapValue::new_unset());
                continue;
            };

            let mut changed = false;
            if force {
                self.insert(key.clone(), value.clone());
                changed = true;
            } else {
                match self.map.get(&key) {
                    Some(old_value) if old_value > &value => {}
                    _ => {
                        self.insert(key.clone(), value.clone());
                        changed = true;
                    }
                }
            }

            if changed {
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
        }

        Diff::Map(resolved_delta)
    }

    fn apply_diff(&mut self, diff: InternalDiff, ctx: DiffApplyContext) {
        let _ = self.apply_diff_and_convert(diff, ctx);
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

    pub fn remove(&mut self, key: &InternalString) {
        let result = self.map.remove(key);
        match result {
            Some(x) => {
                if let Some(_) = x.value {
                    self.size -= 1;
                }
            }
            None => {}
        };
    }

    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, InternalString, MapValue> {
        self.map.iter()
    }

    fn to_map(&self) -> FxHashMap<String, LoroValue> {
        let mut ans = FxHashMap::with_capacity_and_hasher(self.len(), Default::default());
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

    pub fn len(&self) -> usize {
        self.size
    }
}

mod snapshot {
    use fxhash::FxHashSet;
    use loro_common::InternalString;
    use serde_columnar::Itertools;

    use crate::{
        delta::MapValue,
        encoding::value_register::ValueRegister,
        state::{ContainerCreationContext, ContainerState, FastStateSnapshot},
    };

    use super::MapState;

    impl FastStateSnapshot for MapState {
        fn encode_snapshot_fast<W: std::io::prelude::Write>(&mut self, mut w: W) {
            // 1. LoroValue
            // 2. Vec<String> keys_with_none_value
            // 3. leb128 peer_num + peers (in u64)
            // 3. Groups of (leb128 peer_idx, leb128 lamport), each has a respective map entry
            //    from either 1 or 2 when they all sorted by the key strings
            let value = self.get_value();
            postcard::to_io(&value, &mut w).unwrap();

            let keys_with_none_value = self
                .map
                .iter()
                .filter_map(|(k, v)| if v.value.is_some() { None } else { Some(k) })
                .collect_vec();
            postcard::to_io(&keys_with_none_value, &mut w).unwrap();
            let mut peer_register = ValueRegister::new();
            for v in self.map.values() {
                peer_register.register(&v.peer);
            }

            leb128::write::unsigned(&mut w, peer_register.vec().len() as u64).unwrap();
            for p in peer_register.vec() {
                w.write_all(&p.to_le_bytes()).unwrap();
            }
            let mut keys: Vec<&InternalString> = self.map.keys().collect();
            keys.sort_unstable();
            for key in keys.into_iter() {
                let value = self.map.get(key).unwrap();
                let peer_idx = peer_register.register(&value.peer);
                leb128::write::unsigned(&mut w, peer_idx as u64).unwrap();
                leb128::write::unsigned(&mut w, value.lamp as u64).unwrap();
            }
        }

        fn decode_value(bytes: &[u8]) -> loro_common::LoroResult<(loro_common::LoroValue, &[u8])> {
            postcard::take_from_bytes(bytes).map_err(|_| {
                loro_common::LoroError::DecodeError(
                    "Decode map value failed".to_string().into_boxed_str(),
                )
            })
        }

        fn decode_snapshot_fast(
            idx: crate::container::idx::ContainerIdx,
            (value, bytes): (loro_common::LoroValue, &[u8]),
            _ctx: ContainerCreationContext,
        ) -> loro_common::LoroResult<Self>
        where
            Self: Sized,
        {
            let value = value.into_map().unwrap();
            // keys_with_none_value
            let (keys_with_none_value, mut bytes) =
                postcard::take_from_bytes::<Vec<InternalString>>(bytes).map_err(|_| {
                    loro_common::LoroError::DecodeError(
                        "Decode map keys_with_none_value failed"
                            .to_string()
                            .into_boxed_str(),
                    )
                })?;
            let keys_with_none_value: FxHashSet<_> = keys_with_none_value.into_iter().collect();

            // peers
            let peer_count = leb128::read::unsigned(&mut bytes).unwrap() as usize;
            let mut peers = Vec::with_capacity(peer_count);
            for _ in 0..peer_count {
                let peer = u64::from_le_bytes(bytes[..8].try_into().unwrap());
                bytes = &bytes[8..];
                peers.push(peer);
            }

            //
            let mut ans = MapState::new(idx);
            let mut keys: Vec<_> = value.keys().map(|x| x.as_str().into()).collect();
            keys.extend(keys_with_none_value.iter().cloned());
            keys.sort_unstable();

            for key in keys {
                let peer_idx = leb128::read::unsigned(&mut bytes).unwrap() as usize;
                let lamp = leb128::read::unsigned(&mut bytes).unwrap() as u32;
                let peer = peers[peer_idx];

                if keys_with_none_value.contains(&key) {
                    ans.insert(
                        key,
                        MapValue {
                            value: None,
                            lamp,
                            peer,
                        },
                    );
                } else {
                    let value = value.get(&*key).unwrap();
                    ans.insert(
                        key,
                        MapValue {
                            value: Some(value.clone()),
                            lamp,
                            peer,
                        },
                    );
                }
            }

            Ok(ans)
        }
    }

    #[cfg(test)]
    mod map_snapshot_test {
        use loro_common::LoroValue;

        use crate::container::idx::ContainerIdx;

        use super::*;

        #[test]
        fn map_fast_snapshot() {
            let mut map = MapState::new(ContainerIdx::from_index_and_type(
                0,
                loro_common::ContainerType::Map,
            ));
            map.insert(
                "1".into(),
                MapValue {
                    value: None,
                    lamp: 1,
                    peer: 1,
                },
            );
            map.insert(
                "2".into(),
                MapValue {
                    value: Some(LoroValue::I64(0)),
                    lamp: 2,
                    peer: 2,
                },
            );
            map.insert(
                "3".into(),
                MapValue {
                    value: Some(LoroValue::Double(1.0)),
                    lamp: 3,
                    peer: 3,
                },
            );

            let mut bytes = Vec::new();
            map.encode_snapshot_fast(&mut bytes);

            let (value, bytes) = MapState::decode_value(&bytes).unwrap();
            {
                let m = value.clone().into_map().unwrap();
                assert_eq!(m.len(), 2);
                assert_eq!(m.get("2").unwrap(), &LoroValue::I64(0));
                assert_eq!(m.get("3").unwrap(), &LoroValue::Double(1.0));
            }

            let new_map = MapState::decode_snapshot_fast(
                ContainerIdx::from_index_and_type(0, loro_common::ContainerType::Map),
                (value, bytes),
                ContainerCreationContext {
                    configure: &Default::default(),
                    peer: 0,
                },
            )
            .unwrap();
            let v = new_map.map.get(&"2".into()).unwrap();
            assert_eq!(
                v,
                &MapValue {
                    value: Some(LoroValue::I64(0)),
                    lamp: 2,
                    peer: 2,
                }
            );
            let v = new_map.map.get(&"1".into()).unwrap();
            assert_eq!(
                v,
                &MapValue {
                    value: None,
                    lamp: 1,
                    peer: 1,
                }
            );
        }
    }
}
