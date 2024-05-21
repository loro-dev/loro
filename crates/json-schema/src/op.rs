use fractional_index::FractionalIndex;
use loro_common::{ContainerID, IdLp, TreeID, ID};
use serde::{Deserialize, Serialize};

use crate::JsonLoroValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct Op {
    pub content: OpContent,
    #[serde(with = "crate::serde_impl::container")]
    pub container: ContainerID,
    pub counter: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "op")]
pub enum OpContent {
    List(ListOp),
    MovableList(MovableListOp),
    Map(MapOp),
    Text(TextOp),
    Tree(TreeOp),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ListOp {
    Insert {
        pos: usize,
        value: JsonLoroValue,
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
        value: JsonLoroValue,
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
        value: JsonLoroValue,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MapOp {
    Insert { key: String, value: JsonLoroValue },
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
        style: (String, JsonLoroValue),
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

#[cfg(test)]
mod json {
    use loro_common::LoroValue;

    #[test]
    fn json() {
        let op = super::Op {
            counter: 0,
            container: loro_common::ContainerID::Root {
                name: "a".into(),
                container_type: loro_common::ContainerType::List,
            },
            content: super::OpContent::List(super::ListOp::Insert {
                pos: 0,
                value: super::JsonLoroValue(LoroValue::Null),
            }),
        };
        let serialized = serde_json::to_string_pretty(&op).unwrap();
        println!("{}", serialized);
        let _: super::Op = serde_json::from_str(&serialized).unwrap();
    }
}
