use std::sync::Arc;

use enum_as_inner::EnumAsInner;
use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, ContainerType, Counter, InternalString, LoroError, LoroResult, LoroValue, TreeID,
    ID,
};
use serde::{Deserialize, Serialize};

use crate::{
    change::Lamport, container::tree::tree_op::TreeOp,
    encoding::encode_reordered::MAX_COLLECTION_SIZE,
};

use super::arena::{DecodedArenas, EncodedRegisters, EncodedTreeID};

#[derive(Debug)]
pub enum ValueKind {
    Null,          // 0
    True,          // 1
    False,         // 2
    I64,           // 3
    F64,           // 4
    Str,           // 5
    Binary,        // 6
    ContainerType, // 7
    DeleteOnce,    // 8
    DeleteSeq,     // 9
    DeltaInt,      // 10
    LoroValue,     // 11
    MarkStart,     // 12
    TreeMove,      // 13
    ListMove,      // 14
    ListSet,       // 15
    Future(FutureValueKind),
}

#[derive(Debug)]
pub enum LoroValueKind {
    Null,
    True,
    False,
    I64,
    F64,
    Binary,
    Str,
    List,
    Map,
    ContainerType,
}
impl LoroValueKind {
    fn from_u8(kind: u8) -> Self {
        match kind {
            0 => LoroValueKind::Null,
            1 => LoroValueKind::True,
            2 => LoroValueKind::False,
            3 => LoroValueKind::I64,
            4 => LoroValueKind::F64,
            5 => LoroValueKind::Str,
            6 => LoroValueKind::Binary,
            7 => LoroValueKind::List,
            8 => LoroValueKind::Map,
            9 => LoroValueKind::ContainerType,
            _ => unreachable!(),
        }
    }

    fn to_u8(&self) -> u8 {
        match self {
            LoroValueKind::Null => 0,
            LoroValueKind::True => 1,
            LoroValueKind::False => 2,
            LoroValueKind::I64 => 3,
            LoroValueKind::F64 => 4,
            LoroValueKind::Str => 5,
            LoroValueKind::Binary => 6,
            LoroValueKind::List => 7,
            LoroValueKind::Map => 8,
            LoroValueKind::ContainerType => 9,
        }
    }
}

#[derive(Debug)]
pub enum FutureValueKind {
    #[cfg(feature = "counter")]
    Counter, // 16
    Unknown(u8),
}

impl ValueKind {
    pub(super) fn to_u8(&self) -> u8 {
        match self {
            ValueKind::Null => 0,
            ValueKind::True => 1,
            ValueKind::False => 2,
            ValueKind::I64 => 3,
            ValueKind::F64 => 4,
            ValueKind::Str => 5,
            ValueKind::Binary => 6,
            ValueKind::ContainerType => 7,
            ValueKind::DeleteOnce => 8,
            ValueKind::DeleteSeq => 9,
            ValueKind::DeltaInt => 10,
            ValueKind::LoroValue => 11,
            ValueKind::MarkStart => 12,
            ValueKind::TreeMove => 13,
            ValueKind::ListMove => 14,
            ValueKind::ListSet => 15,
            ValueKind::Future(future_value_kind) => match future_value_kind {
                #[cfg(feature = "counter")]
                FutureValueKind::Counter => 16,
                FutureValueKind::Unknown(u8) => *u8 | 0x80,
            },
        }
    }

    pub(super) fn from_u8(mut kind: u8) -> Self {
        kind &= 0x7F;
        match kind {
            0 => ValueKind::Null,
            1 => ValueKind::True,
            2 => ValueKind::False,
            3 => ValueKind::I64,
            4 => ValueKind::F64,
            5 => ValueKind::Str,
            6 => ValueKind::Binary,
            7 => ValueKind::ContainerType,
            8 => ValueKind::DeleteOnce,
            9 => ValueKind::DeleteSeq,
            10 => ValueKind::DeltaInt,
            11 => ValueKind::LoroValue,
            12 => ValueKind::MarkStart,
            13 => ValueKind::TreeMove,
            14 => ValueKind::ListMove,
            15 => ValueKind::ListSet,
            #[cfg(feature = "counter")]
            16 => ValueKind::Future(FutureValueKind::Counter),
            _ => ValueKind::Future(FutureValueKind::Unknown(kind)),
        }
    }
}

#[derive(Debug, EnumAsInner)]
pub enum Value<'a> {
    Null,
    True,
    False,
    I64(i64),
    F64(f64),
    Str(&'a str),
    Binary(&'a [u8]),
    ContainerIdx(usize),
    DeleteOnce,
    DeleteSeq,
    DeltaInt(i32),
    #[allow(clippy::enum_variant_names)]
    LoroValue(LoroValue),
    MarkStart(MarkStart),
    TreeMove(EncodedTreeMove),
    ListMove {
        from: usize,
        from_idx: usize,
        lamport: usize,
    },
    ListSet {
        peer_idx: usize,
        lamport: Lamport,
        value: LoroValue,
    },
    Future(FutureValue<'a>),
}

#[derive(Debug)]
pub enum FutureValue<'a> {
    #[cfg(feature = "counter")]
    Counter,
    // The future value cannot depend on the arena for encoding.
    Unknown {
        kind: u8,
        prop: i32,
        data: &'a [u8],
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OwnedFutureValue {
    #[cfg(feature = "counter")]
    Counter,
    // The future value cannot depend on the arena for encoding.
    Unknown {
        kind: u8,
        prop: i32,
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OwnedValue {
    Null,
    True,
    False,
    I64(i64),
    F64(f64),
    Str(String),
    Binary(Vec<u8>),
    ContainerIdx(usize),
    DeleteOnce,
    DeleteSeq,
    DeltaInt(i32),
    LoroValue(LoroValue),
    MarkStart(MarkStart),
    TreeMove(EncodedTreeMove),
    ListMove {
        from: usize,
        from_idx: usize,
        lamport: usize,
    },
    ListSet {
        peer_idx: usize,
        lamport: Lamport,
        value: LoroValue,
    },
    Future(OwnedFutureValue),
}

impl<'a> Value<'a> {
    pub fn from_owned(owned_value: &'a OwnedValue) -> Self {
        match owned_value {
            OwnedValue::Null => Value::Null,
            OwnedValue::True => Value::True,
            OwnedValue::False => Value::False,
            OwnedValue::DeleteOnce => Value::DeleteOnce,
            OwnedValue::I64(x) => Value::I64(*x),
            OwnedValue::ContainerIdx(x) => Value::ContainerIdx(*x),
            OwnedValue::F64(x) => Value::F64(*x),
            OwnedValue::Str(x) => Value::Str(x.as_str()),
            OwnedValue::DeleteSeq => Value::DeleteSeq,
            OwnedValue::DeltaInt(x) => Value::DeltaInt(*x),
            OwnedValue::LoroValue(x) => Value::LoroValue(x.clone()),
            OwnedValue::MarkStart(x) => Value::MarkStart(x.clone()),
            OwnedValue::Binary(x) => Value::Binary(x.as_slice()),
            OwnedValue::TreeMove(x) => Value::TreeMove(x.clone()),
            OwnedValue::ListMove {
                from,
                from_idx,
                lamport,
            } => Value::ListMove {
                from: *from,
                from_idx: *from_idx,
                lamport: *lamport,
            },
            OwnedValue::ListSet {
                peer_idx,
                lamport,
                value,
            } => Value::ListSet {
                peer_idx: *peer_idx,
                lamport: *lamport,
                value: value.clone(),
            },
            OwnedValue::Future(value) => match value {
                #[cfg(feature = "counter")]
                OwnedFutureValue::Counter => Value::Future(FutureValue::Counter),
                OwnedFutureValue::Unknown { kind, prop, data } => {
                    Value::Future(FutureValue::Unknown {
                        kind: *kind,
                        prop: *prop,
                        data: data.as_slice(),
                    })
                }
            },
        }
    }

    pub fn into_owned(self) -> OwnedValue {
        match self {
            Value::Null => OwnedValue::Null,
            Value::True => OwnedValue::True,
            Value::False => OwnedValue::False,
            Value::DeleteOnce => OwnedValue::DeleteOnce,
            Value::I64(x) => OwnedValue::I64(x),
            Value::ContainerIdx(x) => OwnedValue::ContainerIdx(x),
            Value::F64(x) => OwnedValue::F64(x),
            Value::Str(x) => OwnedValue::Str(x.to_owned()),
            Value::DeleteSeq => OwnedValue::DeleteSeq,
            Value::DeltaInt(x) => OwnedValue::DeltaInt(x),
            Value::LoroValue(x) => OwnedValue::LoroValue(x),
            Value::MarkStart(x) => OwnedValue::MarkStart(x),
            Value::Binary(x) => OwnedValue::Binary(x.to_owned()),
            Value::TreeMove(x) => OwnedValue::TreeMove(x),
            Value::ListMove {
                from,
                from_idx,
                lamport,
            } => OwnedValue::ListMove {
                from,
                from_idx,
                lamport,
            },
            Value::ListSet {
                peer_idx,
                lamport,
                value,
            } => OwnedValue::ListSet {
                peer_idx,
                lamport,
                value,
            },
            Value::Future(value) => match value {
                #[cfg(feature = "counter")]
                FutureValue::Counter => OwnedValue::Future(OwnedFutureValue::Counter),
                FutureValue::Unknown { kind, prop, data } => {
                    OwnedValue::Future(OwnedFutureValue::Unknown {
                        kind,
                        prop,
                        data: data.to_owned(),
                    })
                }
            },
        }
    }

    fn decode_without_arena<'r: 'a>(
        future_kind: FutureValueKind,
        value_reader: &'r mut ValueReader,
        prop: i32,
    ) -> LoroResult<Self> {
        let bytes_length = value_reader.read_usize()?;
        let value = match future_kind {
            #[cfg(feature = "counter")]
            FutureValueKind::Counter => FutureValue::Counter,
            FutureValueKind::Unknown(kind) => FutureValue::Unknown {
                kind,
                prop,
                data: value_reader.take_bytes(bytes_length),
            },
        };
        Ok(Value::Future(value))
    }

    pub(super) fn decode<'r: 'a>(
        kind: ValueKind,
        value_reader: &'r mut ValueReader,
        arenas: &'a DecodedArenas<'a>,
        id: ID,
        prop: i32,
    ) -> LoroResult<Self> {
        Ok(match kind {
            ValueKind::Null => Value::Null,
            ValueKind::True => Value::True,
            ValueKind::False => Value::False,
            ValueKind::I64 => Value::I64(value_reader.read_i64()?),
            ValueKind::F64 => Value::F64(value_reader.read_f64()?),
            ValueKind::Str => Value::Str(value_reader.read_str()?),
            ValueKind::Binary => Value::Binary(value_reader.read_binary()?),
            ValueKind::ContainerType => Value::ContainerIdx(value_reader.read_usize()?),
            ValueKind::DeleteOnce => Value::DeleteOnce,
            ValueKind::DeleteSeq => Value::DeleteSeq,
            ValueKind::DeltaInt => Value::DeltaInt(value_reader.read_i32()?),
            ValueKind::LoroValue => {
                Value::LoroValue(value_reader.read_value_type_and_content(&arenas.keys, id)?)
            }
            ValueKind::MarkStart => {
                Value::MarkStart(value_reader.read_mark(&arenas.keys.keys, id)?)
            }
            ValueKind::TreeMove => Value::TreeMove(value_reader.read_tree_move()?),
            ValueKind::ListMove => {
                let from = value_reader.read_usize()?;
                let from_idx = value_reader.read_usize()?;
                let lamport = value_reader.read_usize()?;
                Value::ListMove {
                    from,
                    from_idx,
                    lamport,
                }
            }
            ValueKind::ListSet => {
                let peer_idx = value_reader.read_usize()?;
                let lamport = value_reader.read_usize()? as u32;
                let value = value_reader.read_value_type_and_content(&arenas.keys.keys, id)?;
                Value::ListSet {
                    peer_idx,
                    lamport,
                    value,
                }
            }
            ValueKind::Future(future_kind) => {
                Self::decode_without_arena(future_kind, value_reader, prop)?
            }
        })
    }

    fn encode_without_registers(
        value: FutureValue,
        value_writer: &mut ValueWriter,
    ) -> (FutureValueKind, usize) {
        match value {
            #[cfg(feature = "counter")]
            FutureValue::Counter => {
                // write bytes length
                value_writer.write_u8(0);
                (FutureValueKind::Counter, 0)
            }
            FutureValue::Unknown {
                kind,
                prop: _,
                data,
            } => (
                FutureValueKind::Unknown(kind),
                value_writer.write_binary(data),
            ),
        }
    }

    pub(super) fn encode(
        self,
        value_writer: &mut ValueWriter,
        registers: &mut EncodedRegisters,
    ) -> (ValueKind, usize) {
        match self {
            Value::Null => (ValueKind::Null, 0),
            Value::True => (ValueKind::True, 0),
            Value::False => (ValueKind::False, 0),
            Value::I64(x) => (ValueKind::I64, value_writer.write_i64(x)),
            Value::F64(x) => (ValueKind::F64, value_writer.write_f64(x)),
            Value::Str(x) => (ValueKind::Str, value_writer.write_str(x)),
            Value::Binary(x) => (ValueKind::Binary, value_writer.write_binary(x)),
            Value::ContainerIdx(x) => (ValueKind::ContainerType, value_writer.write_usize(x)),
            Value::DeleteOnce => (ValueKind::DeleteOnce, 0),
            Value::DeleteSeq => (ValueKind::DeleteSeq, 0),
            Value::DeltaInt(x) => (ValueKind::DeltaInt, value_writer.write_i32(x)),
            Value::LoroValue(x) => (
                ValueKind::LoroValue,
                value_writer.write_value_type_and_content(&x, registers),
            ),
            Value::MarkStart(x) => (ValueKind::MarkStart, value_writer.write_mark(x, registers)),
            Value::TreeMove(tree) => (ValueKind::TreeMove, value_writer.write_tree_move(&tree)),
            Value::ListMove {
                from,
                from_idx,
                lamport,
            } => (
                ValueKind::ListMove,
                value_writer.write_usize(from)
                    + value_writer.write_usize(from_idx)
                    + value_writer.write_usize(lamport),
            ),
            Value::ListSet {
                peer_idx,
                lamport,
                value,
            } => (
                ValueKind::ListSet,
                value_writer.write_usize(peer_idx)
                    + value_writer.write_usize(lamport as usize)
                    + value_writer.write_value_type_and_content(&value, registers),
            ),
            Value::Future(value) => {
                let (k, i) = Self::encode_without_registers(value, value_writer);
                (ValueKind::Future(k), i)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkStart {
    pub len: u32,
    pub key: InternalString,
    pub value: LoroValue,
    pub info: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedTreeMove {
    pub subject_idx: usize,
    pub is_parent_null: bool,
    pub parent_idx: usize,
    pub position: usize,
}

impl EncodedTreeMove {
    pub fn as_tree_op(
        &self,
        peer_ids: &[u64],
        positions: &[Vec<u8>],
        tree_ids: &[EncodedTreeID],
    ) -> LoroResult<TreeOp> {
        let parent = if self.is_parent_null {
            None
        } else {
            let EncodedTreeID { peer_idx, counter } = tree_ids[self.parent_idx];
            Some(TreeID::new(
                *(peer_ids
                    .get(peer_idx)
                    .ok_or(LoroError::DecodeDataCorruptionError)?),
                counter as Counter,
            ))
        };
        let position = if parent.is_some_and(|x| TreeID::is_deleted_root(&x)) {
            None
        } else {
            let bytes = &positions[self.position];
            Some(FractionalIndex::from_bytes(bytes.clone()))
        };
        let EncodedTreeID { peer_idx, counter } = tree_ids[self.subject_idx];
        Ok(TreeOp {
            target: TreeID::new(
                *(peer_ids
                    .get(peer_idx)
                    .ok_or(LoroError::DecodeDataCorruptionError)?),
                counter as Counter,
            ),
            parent,
            position,
        })
    }

    pub fn from_tree_op<'p, 'a: 'p>(op: &'a TreeOp, registers: &mut EncodedRegisters) -> Self {
        let position = if let Some(position) = &op.position {
            let bytes = position.as_bytes();
            let either::Right(position_register) = &mut registers.position else {
                unreachable!()
            };
            position_register.get(&bytes).unwrap()
        } else {
            debug_assert!(op.parent.is_some_and(|x| TreeID::is_deleted_root(&x)));
            // placeholder
            0
        };

        let target_idx = registers.tree_id.register(&EncodedTreeID {
            peer_idx: registers.peer.register(&op.target.peer),
            counter: op.target.counter,
        });

        let parent_idx = op.parent.map(|x| {
            registers.tree_id.register(&EncodedTreeID {
                peer_idx: registers.peer.register(&x.peer),
                counter: x.counter,
            })
        });

        EncodedTreeMove {
            subject_idx: target_idx,
            is_parent_null: op.parent.is_none(),
            parent_idx: parent_idx.unwrap_or(0),
            position,
        }
    }
}

pub struct ValueWriter {
    buffer: Vec<u8>,
}

pub struct ValueReader<'a> {
    raw: &'a [u8],
}

impl<'a> ValueReader<'a> {
    pub fn new(raw: &'a [u8]) -> Self {
        ValueReader { raw }
    }

    pub fn read_value_type_and_content(
        &mut self,
        keys: &[InternalString],
        id: ID,
    ) -> LoroResult<LoroValue> {
        let kind = self.read_u8()?;
        self.read_value_content(LoroValueKind::from_u8(kind), keys, id)
    }

    pub fn read_value_content(
        &mut self,
        kind: LoroValueKind,
        keys: &[InternalString],
        id: ID,
    ) -> LoroResult<LoroValue> {
        Ok(match kind {
            LoroValueKind::Null => LoroValue::Null,
            LoroValueKind::True => LoroValue::Bool(true),
            LoroValueKind::False => LoroValue::Bool(false),
            LoroValueKind::I64 => LoroValue::I64(self.read_i64()?),
            LoroValueKind::F64 => LoroValue::Double(self.read_f64()?),
            LoroValueKind::Str => LoroValue::String(Arc::new(self.read_str()?.to_owned())),
            LoroValueKind::List => {
                let len = self.read_usize()?;
                if len > MAX_COLLECTION_SIZE {
                    return Err(LoroError::DecodeDataCorruptionError);
                }
                let mut ans = Vec::with_capacity(len);
                for i in 0..len {
                    ans.push(self.recursive_read_value_type_and_content(keys, id.inc(i as i32))?);
                }
                ans.into()
            }
            LoroValueKind::Map => {
                let len = self.read_usize()?;
                if len > MAX_COLLECTION_SIZE {
                    return Err(LoroError::DecodeDataCorruptionError);
                }
                let mut ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
                for _ in 0..len {
                    let key_idx = self.read_usize()?;
                    let key = keys
                        .get(key_idx)
                        .ok_or(LoroError::DecodeDataCorruptionError)?
                        .to_string();
                    let value = self.recursive_read_value_type_and_content(keys, id)?;
                    ans.insert(key, value);
                }
                ans.into()
            }
            LoroValueKind::Binary => LoroValue::Binary(Arc::new(self.read_binary()?.to_owned())),
            LoroValueKind::ContainerType => {
                let u8 = self.read_u8()?;
                let container_id = ContainerID::new_normal(
                    id,
                    ContainerType::try_from_u8(u8).unwrap_or(ContainerType::Unknown(u8)),
                );

                LoroValue::Container(container_id)
            }
        })
    }

    /// Read a value that may be very deep efficiently.
    ///
    /// This method avoids using recursive calls to read deeply nested values.
    /// Otherwise, it may cause stack overflow.
    fn recursive_read_value_type_and_content(
        &mut self,
        keys: &[InternalString],
        id: ID,
    ) -> LoroResult<LoroValue> {
        #[derive(Debug)]
        enum Task {
            Init,
            ReadList {
                left: usize,
                vec: Vec<LoroValue>,
                key_idx_in_parent: usize,
            },
            ReadMap {
                left: usize,
                map: FxHashMap<String, LoroValue>,
                key_idx_in_parent: usize,
            },
        }
        impl Task {
            fn should_read(&self) -> bool {
                !matches!(
                    self,
                    Self::ReadList { left: 0, .. } | Self::ReadMap { left: 0, .. }
                )
            }

            fn key_idx(&self) -> usize {
                match self {
                    Self::ReadList {
                        key_idx_in_parent, ..
                    } => *key_idx_in_parent,
                    Self::ReadMap {
                        key_idx_in_parent, ..
                    } => *key_idx_in_parent,
                    _ => unreachable!(),
                }
            }

            fn into_value(self) -> LoroValue {
                match self {
                    Self::ReadList { vec, .. } => vec.into(),
                    Self::ReadMap { map, .. } => map.into(),
                    _ => unreachable!(),
                }
            }
        }
        let mut stack = vec![Task::Init];
        while let Some(mut task) = stack.pop() {
            if task.should_read() {
                let key_idx = if matches!(task, Task::ReadMap { .. }) {
                    self.read_usize()?
                } else {
                    0
                };
                let kind = self.read_u8()?;
                let kind = LoroValueKind::from_u8(kind);
                let value = match kind {
                    LoroValueKind::Null => LoroValue::Null,
                    LoroValueKind::True => LoroValue::Bool(true),
                    LoroValueKind::False => LoroValue::Bool(false),
                    LoroValueKind::I64 => LoroValue::I64(self.read_i64()?),
                    LoroValueKind::F64 => LoroValue::Double(self.read_f64()?),
                    LoroValueKind::Str => LoroValue::String(Arc::new(self.read_str()?.to_owned())),
                    LoroValueKind::List => {
                        let len = self.read_usize()?;
                        if len > MAX_COLLECTION_SIZE {
                            return Err(LoroError::DecodeDataCorruptionError);
                        }
                        let ans = Vec::with_capacity(len);
                        stack.push(task);
                        stack.push(Task::ReadList {
                            left: len,
                            vec: ans,
                            key_idx_in_parent: key_idx,
                        });
                        continue;
                    }
                    LoroValueKind::Map => {
                        let len = self.read_usize()?;
                        if len > MAX_COLLECTION_SIZE {
                            return Err(LoroError::DecodeDataCorruptionError);
                        }

                        let ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
                        stack.push(task);
                        stack.push(Task::ReadMap {
                            left: len,
                            map: ans,
                            key_idx_in_parent: key_idx,
                        });
                        continue;
                    }
                    LoroValueKind::Binary => {
                        LoroValue::Binary(Arc::new(self.read_binary()?.to_owned()))
                    }
                    LoroValueKind::ContainerType => {
                        let u8 = self.read_u8()?;
                        let container_id = ContainerID::new_normal(
                            id,
                            ContainerType::try_from_u8(u8).unwrap_or(ContainerType::Unknown(u8)),
                        );

                        LoroValue::Container(container_id)
                    }
                };

                task = match task {
                    Task::Init => {
                        return Ok(value);
                    }
                    Task::ReadList {
                        mut left,
                        mut vec,
                        key_idx_in_parent,
                    } => {
                        left -= 1;
                        vec.push(value);
                        let task = Task::ReadList {
                            left,
                            vec,
                            key_idx_in_parent,
                        };
                        if left != 0 {
                            stack.push(task);
                            continue;
                        }

                        task
                    }
                    Task::ReadMap {
                        mut left,
                        mut map,
                        key_idx_in_parent,
                    } => {
                        left -= 1;
                        let key = keys
                            .get(key_idx)
                            .ok_or(LoroError::DecodeDataCorruptionError)?
                            .to_string();
                        map.insert(key, value);
                        let task = Task::ReadMap {
                            left,
                            map,
                            key_idx_in_parent,
                        };
                        if left != 0 {
                            stack.push(task);
                            continue;
                        }
                        task
                    }
                };
            }

            let key_index = task.key_idx();
            let value = task.into_value();
            if let Some(last) = stack.last_mut() {
                match last {
                    Task::Init => {
                        return Ok(value);
                    }
                    Task::ReadList { left, vec, .. } => {
                        *left -= 1;
                        vec.push(value);
                    }
                    Task::ReadMap { left, map, .. } => {
                        *left -= 1;
                        let key = keys
                            .get(key_index)
                            .ok_or(LoroError::DecodeDataCorruptionError)?
                            .to_string();
                        map.insert(key, value);
                    }
                }
            } else {
                return Ok(value);
            }
        }

        unreachable!();
    }

    pub fn read_i64(&mut self) -> LoroResult<i64> {
        leb128::read::signed(&mut self.raw).map_err(|_| LoroError::DecodeDataCorruptionError)
    }

    #[allow(unused)]
    pub fn read_u64(&mut self) -> LoroResult<u64> {
        leb128::read::unsigned(&mut self.raw).map_err(|_| LoroError::DecodeDataCorruptionError)
    }

    #[allow(unused)]
    pub fn read_i32(&mut self) -> LoroResult<i32> {
        leb128::read::signed(&mut self.raw)
            .map(|x| x as i32)
            .map_err(|_| LoroError::DecodeDataCorruptionError)
    }

    fn read_f64(&mut self) -> LoroResult<f64> {
        if self.raw.len() < 8 {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let mut bytes = [0; 8];
        bytes.copy_from_slice(&self.raw[..8]);
        self.raw = &self.raw[8..];
        Ok(f64::from_be_bytes(bytes))
    }

    pub fn read_usize(&mut self) -> LoroResult<usize> {
        Ok(leb128::read::unsigned(&mut self.raw)
            .map_err(|_| LoroError::DecodeDataCorruptionError)? as usize)
    }

    pub fn read_str(&mut self) -> LoroResult<&'a str> {
        let len = self.read_usize()?;
        if self.raw.len() < len {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let ans = std::str::from_utf8(&self.raw[..len]).unwrap();
        self.raw = &self.raw[len..];
        Ok(ans)
    }

    fn read_u8(&mut self) -> LoroResult<u8> {
        if self.raw.is_empty() {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let ans = self.raw[0];
        self.raw = &self.raw[1..];
        Ok(ans)
    }

    pub fn read_binary(&mut self) -> LoroResult<&'a [u8]> {
        let len = self.read_usize()?;
        if self.raw.len() < len {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let ans = &self.raw[..len];
        self.raw = &self.raw[len..];
        Ok(ans)
    }

    pub fn read_mark<'s: 'm, 'm>(
        &mut self,
        keys: &'s [InternalString],
        id: ID,
    ) -> LoroResult<MarkStart> {
        let info = self.read_u8()?;
        let len = self.read_usize()?;
        let key_idx = self.read_usize()?;
        let value = self.read_value_type_and_content(keys, id)?;
        Ok(MarkStart {
            len: len as u32,
            key: keys
                .get(key_idx)
                .ok_or(LoroError::DecodeDataCorruptionError)?
                .clone(),
            value,
            info,
        })
    }

    pub fn take_bytes(&mut self, len: usize) -> &'a [u8] {
        let ans = &self.raw[..len];
        self.raw = &self.raw[len..];
        ans
    }

    pub fn read_tree_move(&mut self) -> LoroResult<EncodedTreeMove> {
        let subject_idx = self.read_usize()?;
        let is_parent_null = self.read_u8()? != 0;
        let position = self.read_usize()?;
        let mut parent_idx = 0;
        if !is_parent_null {
            parent_idx = self.read_usize()?;
        }
        Ok(EncodedTreeMove {
            subject_idx,
            is_parent_null,
            parent_idx,
            position,
        })
    }
}

impl ValueWriter {
    pub fn new() -> Self {
        ValueWriter { buffer: Vec::new() }
    }

    pub fn write_value_type_and_content(
        &mut self,
        value: &LoroValue,
        registers: &mut EncodedRegisters,
    ) -> usize {
        let len = self.write_u8(get_loro_value_kind(value).to_u8());
        let (_, l) = self.write_value_content(value, registers);
        len + l
    }

    pub fn write_value_content(
        &mut self,
        value: &LoroValue,
        registers: &mut EncodedRegisters,
    ) -> (LoroValueKind, usize) {
        match value {
            LoroValue::Null => (LoroValueKind::Null, 0),
            LoroValue::Bool(true) => (LoroValueKind::True, 0),
            LoroValue::Bool(false) => (LoroValueKind::False, 0),
            LoroValue::I64(value) => (LoroValueKind::I64, self.write_i64(*value)),
            LoroValue::Double(value) => (LoroValueKind::F64, self.write_f64(*value)),
            LoroValue::String(value) => (LoroValueKind::Str, self.write_str(value)),
            LoroValue::List(value) => {
                let mut len = self.write_usize(value.len());
                for value in value.iter() {
                    let l = self.write_value_type_and_content(value, registers);
                    len += l;
                }
                (LoroValueKind::List, len)
            }
            LoroValue::Map(value) => {
                let mut len = self.write_usize(value.len());
                for (key, value) in value.iter() {
                    let key_idx = registers.key.register(&key.as_str().into());
                    len += self.write_usize(key_idx);
                    let l = self.write_value_type_and_content(value, registers);
                    len += l;
                }
                (LoroValueKind::Map, len)
            }
            LoroValue::Binary(value) => (LoroValueKind::Binary, self.write_binary(value)),
            LoroValue::Container(c) => (
                LoroValueKind::ContainerType,
                self.write_u8(c.container_type().to_u8()),
            ),
        }
    }

    pub fn write_i64(&mut self, value: i64) -> usize {
        let len = self.buffer.len();
        leb128::write::signed(&mut self.buffer, value).unwrap();
        self.buffer.len() - len
    }

    fn write_i32(&mut self, value: i32) -> usize {
        let len = self.buffer.len();
        leb128::write::signed(&mut self.buffer, value as i64).unwrap();
        self.buffer.len() - len
    }

    #[allow(unused)]
    fn write_u64(&mut self, value: u64) -> usize {
        let len = self.buffer.len();
        leb128::write::unsigned(&mut self.buffer, value).unwrap();
        self.buffer.len() - len
    }

    fn write_usize(&mut self, value: usize) -> usize {
        let len = self.buffer.len();
        leb128::write::unsigned(&mut self.buffer, value as u64).unwrap();
        self.buffer.len() - len
    }

    fn write_f64(&mut self, value: f64) -> usize {
        let len = self.buffer.len();
        self.buffer.extend_from_slice(&value.to_be_bytes());
        self.buffer.len() - len
    }

    fn write_str(&mut self, value: &str) -> usize {
        let len = self.buffer.len();
        self.write_usize(value.len());
        self.buffer.extend_from_slice(value.as_bytes());
        self.buffer.len() - len
    }

    fn write_u8(&mut self, value: u8) -> usize {
        let len = self.buffer.len();
        self.buffer.push(value);
        self.buffer.len() - len
    }

    fn write_binary(&mut self, value: &[u8]) -> usize {
        let len = self.buffer.len();
        self.write_usize(value.len());
        self.buffer.extend_from_slice(value);
        self.buffer.len() - len
    }

    fn write_mark(&mut self, mark: MarkStart, registers: &mut EncodedRegisters) -> usize {
        let key_idx = registers.key.register(&mark.key);
        let len = self.buffer.len();
        self.write_u8(mark.info);
        self.write_usize(mark.len as usize);
        self.write_usize(key_idx);
        self.write_value_type_and_content(&mark.value, registers);
        self.buffer.len() - len
    }

    fn write_tree_move(&mut self, op: &EncodedTreeMove) -> usize {
        let len = self.buffer.len();
        self.write_usize(op.subject_idx);
        self.write_u8(op.is_parent_null as u8);
        self.write_usize(op.position);
        if op.is_parent_null {
            return self.buffer.len() - len;
        }
        self.write_usize(op.parent_idx);
        self.buffer.len() - len
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

fn get_loro_value_kind(value: &LoroValue) -> LoroValueKind {
    match value {
        LoroValue::Null => LoroValueKind::Null,
        LoroValue::Bool(true) => LoroValueKind::True,
        LoroValue::Bool(false) => LoroValueKind::False,
        LoroValue::I64(_) => LoroValueKind::I64,
        LoroValue::Double(_) => LoroValueKind::F64,
        LoroValue::String(_) => LoroValueKind::Str,
        LoroValue::List(_) => LoroValueKind::List,
        LoroValue::Map(_) => LoroValueKind::Map,
        LoroValue::Binary(_) => LoroValueKind::Binary,
        LoroValue::Container(_) => LoroValueKind::ContainerType,
    }
}
