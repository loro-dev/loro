use std::borrow::Cow;

use fractional_index::FractionalIndex;
use loro_common::{ContainerID, IdLp, Lamport, LoroValue, PeerID, TreeID, ID};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{encoding::OwnedValue, VersionVector};

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonSchema<'a> {
    pub schema_version: u8,
    pub start_vv: VersionVector,
    pub end_vv: VersionVector,
    pub peers: Vec<PeerID>,
    pub changes: Vec<Change<'a>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Change<'a> {
    #[serde(with = "self::serde_impl::id")]
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
    // #[serde(with = "self::serde_impl::future_op")]
    Future(FutureOpWrapper<'a>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FutureOpWrapper<'a> {
    #[serde(flatten)]
    pub value: FutureOp<'a>,
    pub prop: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListOp {
    Insert {
        pos: usize,
        value: LoroValue,
    },
    Delete {
        pos: isize,
        len: isize,
        #[serde(with = "self::serde_impl::id")]
        delete_start_id: ID,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MovableListOp {
    Insert {
        pos: usize,
        value: LoroValue,
    },
    Delete {
        pos: isize,
        len: isize,
        #[serde(with = "self::serde_impl::id")]
        delete_start_id: ID,
    },
    Move {
        from: u32,
        to: u32,
        #[serde(with = "self::serde_impl::idlp")]
        from_id: IdLp,
    },
    Set {
        #[serde(with = "self::serde_impl::idlp")]
        elem_id: IdLp,
        value: LoroValue,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MapOp {
    Insert { key: String, value: LoroValue },
    Delete { key: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextOp {
    Insert {
        pos: u32,
        text: String,
    },
    Delete {
        pos: isize,
        len: isize,
        #[serde(with = "self::serde_impl::id")]
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeOp {
    // Create {
    //     target: TreeID,
    //     parent: Option<TreeID>,
    //     fractional_index: String,
    // },
    Move {
        target: TreeID,
        parent: Option<TreeID>,
        #[serde(default)]
        fractional_index: FractionalIndex,
    },
    Delete {
        target: TreeID,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FutureOp<'a> {
    #[cfg(feature = "counter")]
    Counter(Cow<'a, OwnedValue>),
    Unknown(Cow<'a, OwnedValue>),
}

mod serde_impl {

    use loro_common::{ContainerID, ContainerType};
    use serde::{
        de::{MapAccess, Visitor},
        ser::SerializeStruct,
        Deserialize, Deserializer, Serialize, Serializer,
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
                    A: MapAccess<'de>,
                {
                    let (_key, container) = map.next_entry::<&str, &str>()?.unwrap();
                    let is_unknown = container.ends_with(')');
                    let container = ContainerID::try_from(container)
                        .map_err(|_| serde::de::Error::custom("invalid container id"))?;
                    let op = if is_unknown {
                        let (_key, op) = map.next_entry::<&str, super::FutureOpWrapper>()?.unwrap();
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
                                let (_key, v) = map.next_entry::<&str, i64>()?.unwrap();
                                super::OpContent::Future(super::FutureOpWrapper {
                                    prop: v as i32,
                                    value: super::FutureOp::Counter(std::borrow::Cow::Owned(
                                        crate::encoding::value::OwnedValue::Future(
                                            crate::encoding::future_value::OwnedFutureValue::Counter,
                                        ),
                                    )),
                                })
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

        use serde::{Deserialize, Deserializer};

        use crate::encoding::json_schema::op::FutureOp;

        impl<'de, 'a> Deserialize<'de> for FutureOp<'a> {
            fn deserialize<D>(d: D) -> Result<FutureOp<'a>, D::Error>
            where
                D: Deserializer<'de>,
            {
                enum Field {
                    #[cfg(feature = "counter")]
                    Counter,
                    Unknown,
                }
                struct FieldVisitor;
                impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                    type Value = Field;
                    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                        f.write_str("field identifier")
                    }
                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            #[cfg(feature = "counter")]
                            "counter" => Ok(Field::Counter),
                            _ => Ok(Field::Unknown),
                        }
                    }
                }
                impl<'de> Deserialize<'de> for Field {
                    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        deserializer.deserialize_identifier(FieldVisitor)
                    }
                }
                let (tag, content) =
                    d.deserialize_any(serde::__private::de::TaggedContentVisitor::<Field>::new(
                        "type",
                        "internally tagged enum FutureOp",
                    ))?;
                let __deserializer =
                    serde::__private::de::ContentDeserializer::<D::Error>::new(content);
                match tag {
                    #[cfg(feature = "counter")]
                    Field::Counter => {
                        let v = serde::Deserialize::deserialize(__deserializer)?;
                        Ok(FutureOp::Counter(v))
                    }
                    _ => {
                        let v = serde::Deserialize::deserialize(__deserializer)?;
                        Ok(FutureOp::Unknown(v))
                    }
                }
            }
        }
    }

    pub mod id {
        use loro_common::ID;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(id: &ID, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            s.serialize_str(&id.to_string())
        }

        pub fn deserialize<'de, 'a, D>(d: D) -> Result<ID, D::Error>
        where
            D: Deserializer<'de>,
        {
            let str: &str = Deserialize::deserialize(d)?;
            let id: ID = ID::try_from(str).unwrap();
            Ok(id)
        }
    }

    pub mod idlp {
        use loro_common::IdLp;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(idlp: &IdLp, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            s.serialize_str(&idlp.to_string())
        }

        pub fn deserialize<'de, 'a, D>(d: D) -> Result<IdLp, D::Error>
        where
            D: Deserializer<'de>,
        {
            let str: &str = Deserialize::deserialize(d)?;
            let id: IdLp = IdLp::try_from(str).unwrap();
            Ok(id)
        }
    }
}
