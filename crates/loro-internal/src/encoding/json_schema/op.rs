use std::borrow::Cow;

use fractional_index::FractionalIndex;
use loro_common::{ContainerID, IdLp, Lamport, LoroValue, TreeID, ID};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::encoding::OwnedValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonSchema<'a> {
    pub loro_version: String,
    pub start_vv: String,
    pub end_vv: String,
    pub changes: Vec<Change<'a>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Change<'a> {
    pub id: ID,
    pub timestamp: i64,
    pub deps: SmallVec<[ID; 2]>,
    pub lamport: Lamport,
    pub msg: Option<String>,
    pub ops: Vec<Op<'a>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Op<'a> {
    pub content: OpContent<'a>,
    #[serde(with = "self::serde_impl::container")]
    pub container: ContainerID,
    pub counter: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "op")]
pub enum OpContent<'a> {
    List(ListOp),
    MovableList(MovableListOp),
    Map(MapOp),
    Text(TextOp),
    Tree(TreeOp),
    #[serde(with = "self::serde_impl::future_op")]
    Future(FutureOp<'a>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ListOp {
    Insert {
        pos: usize,
        value: LoroValue,
    },
    Delete {
        pos: isize,
        len: isize,
        delete_start_id: ID,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MovableListOp {
    Insert {
        pos: usize,
        value: LoroValue,
    },
    Delete {
        pos: isize,
        len: isize,
        delete_start_id: ID,
    },
    Move {
        from: u32,
        to: u32,
        from_id: IdLp,
    },
    Set {
        elem_id: IdLp,
        value: LoroValue,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MapOp {
    Insert { key: String, value: LoroValue },
    Delete { key: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TextOp {
    Insert {
        pos: u32,
        text: String,
    },
    Delete {
        pos: isize,
        len: isize,
        id_start: ID,
    },
    Mark {
        start: u32,
        end: u32,
        style: (String, LoroValue),
        info: u8,
    },
    MarkEnd,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TreeOp {
    // Create {
    //     target: TreeID,
    //     parent: Option<TreeID>,
    //     fractional_index: String,
    // },
    Move {
        target: TreeID,
        parent: Option<TreeID>,
        fractional_index: FractionalIndex,
    },
    Delete {
        target: TreeID,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "op")]
pub enum FutureOp<'a> {
    #[cfg(feature = "counter")]
    Counter(i64),
    Unknown {
        prop: i32,
        value: Cow<'a, OwnedValue>,
    },
}

mod serde_impl {
    pub mod container {
        use loro_common::ContainerID;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(container: &ContainerID, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            s.serialize_str(container.to_string().as_str())
        }

        pub fn deserialize<'de, D>(d: D) -> Result<ContainerID, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(d)?;
            ContainerID::try_from(s.as_str())
                .map_err(|_| serde::de::Error::custom("invalid container id"))
        }
    }

    pub mod future_op {

        use serde::{Deserialize, Deserializer, Serializer};

        use crate::encoding::json_schema::op::FutureOp;

        pub fn serialize<S>(op: &FutureOp<'_>, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let json_str = serde_json::to_string_pretty(op).unwrap();
            s.serialize_str(&json_str)
        }

        pub fn deserialize<'de, 'a, D>(d: D) -> Result<FutureOp<'a>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let str: &str = Deserialize::deserialize(d)?;
            let future_op: FutureOp =
                serde_json::from_str(str).map_err(serde::de::Error::custom)?;
            Ok(future_op)
        }
    }
}
