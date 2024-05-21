use std::borrow::Cow;

use fractional_index::FractionalIndex;
use loro_common::{ContainerID, IdLp, Lamport, LoroValue, TreeID, ID};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::encoding::OwnedValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonSchema<'a> {
    pub loro_version: &'a str,
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

#[derive(Debug)]
pub struct Op<'a> {
    pub content: OpContent<'a>,
    pub container: ContainerID,
    pub counter: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
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
#[serde(tag = "type", rename_all = "camelCase")]
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
#[serde(tag = "type", rename_all = "camelCase")]
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
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MapOp {
    Insert { key: String, value: LoroValue },
    Delete { key: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
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
#[serde(tag = "type", rename_all = "camelCase")]
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
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FutureOp<'a> {
    #[cfg(feature = "counter")]
    Counter(i64),
    Unknown {
        prop: i32,
        value: Cow<'a, OwnedValue>,
    },
}

mod serde_impl {
    use loro_common::{ContainerID, ContainerType};
    use serde::{
        de::Visitor, ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer,
    };

    impl<'a> Serialize for super::Op<'a> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut s = serializer.serialize_struct("Op", 3)?;
            s.serialize_field("container", &self.container.to_string())?;
            s.serialize_field("content", &self.content)?;
            s.serialize_field("counter", &self.counter)?;
            s.end()
        }
    }

    impl<'a, 'de> Deserialize<'de> for super::Op<'a> {
        fn deserialize<D>(deserializer: D) -> Result<super::Op<'static>, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct __Visitor<'a> {
                marker: std::marker::PhantomData<super::Op<'a>>,
            }

            impl<'a, 'de> Visitor<'de> for __Visitor<'a> {
                type Value = super::Op<'a>;
                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("struct Op")
                }

                fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: serde::de::MapAccess<'de>,
                {
                    let (_key, container) = map.next_entry::<&str, &str>()?.unwrap();
                    let is_unknown = container.ends_with(')');
                    let container = ContainerID::try_from(container)
                        .map_err(|_| serde::de::Error::custom("invalid container id"))?;
                    let op = if is_unknown {
                        let (_key, op) = map.next_entry::<&str, super::FutureOp>()?.unwrap();
                        super::OpContent::Future(op)
                    } else {
                        match container.container_type() {
                            ContainerType::List => {
                                let (_key, op) = map.next_entry::<&str, super::ListOp>()?.unwrap();
                                super::OpContent::List(op)
                            }
                            ContainerType::MovableList => {
                                let (_key, op) =
                                    map.next_entry::<&str, super::MovableListOp>()?.unwrap();
                                super::OpContent::MovableList(op)
                            }
                            ContainerType::Map => {
                                let (_key, op) = map.next_entry::<&str, super::MapOp>()?.unwrap();
                                super::OpContent::Map(op)
                            }
                            ContainerType::Text => {
                                let (_key, op) = map.next_entry::<&str, super::TextOp>()?.unwrap();
                                super::OpContent::Text(op)
                            }
                            ContainerType::Tree => {
                                let (_key, op) = map.next_entry::<&str, super::TreeOp>()?.unwrap();
                                super::OpContent::Tree(op)
                            }
                            #[cfg(feature = "counter")]
                            ContainerType::Counter => {
                                let (_key, op) = map.next_entry::<&str, i64>()?.unwrap();
                                super::OpContent::Future(super::FutureOp::Counter(op))
                            }
                            _ => unreachable!(),
                        }
                    };
                    let (_, counter) = map.next_entry::<&str, i32>()?.unwrap();
                    Ok(super::Op {
                        container,
                        content: op,
                        counter,
                    })
                }
            }
            const FIELDS: &[&str] = &["content", "container", "counter"];
            deserializer.deserialize_struct(
                "Op",
                FIELDS,
                __Visitor {
                    marker: Default::default(),
                },
            )
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
