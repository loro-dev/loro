use std::collections::HashMap;

use loro::{Counter, PeerID};

pub trait LoroValueLike: Sync + Send {
    fn as_loro_value(&self) -> crate::LoroValue;
}

#[derive(Debug, Clone, Copy)]
pub enum ContainerType {
    Text,
    Map,
    List,
    MovableList,
    Tree,
    Counter,
    Unknown { kind: u8 },
}

#[derive(Debug, Clone)]
pub enum ContainerID {
    Root {
        name: String,
        container_type: ContainerType,
    },
    Normal {
        peer: PeerID,
        counter: Counter,
        container_type: ContainerType,
    },
}

#[derive(Debug, Clone)]
pub enum LoroValue {
    Null,
    Bool { value: bool },
    Double { value: f64 },
    I64 { value: i64 },
    Binary { value: Vec<u8> },
    String { value: String },
    List { value: Vec<LoroValue> },
    Map { value: HashMap<String, LoroValue> },
    Container { value: ContainerID },
}

impl From<LoroValue> for loro::LoroValue {
    fn from(value: LoroValue) -> Self {
        match value {
            LoroValue::Null => Self::Null,
            LoroValue::Bool { value } => Self::Bool(value),
            LoroValue::Double { value } => Self::Double(value),
            LoroValue::I64 { value } => Self::I64(value),
            LoroValue::Binary { value } => Self::Binary(value.into()),
            LoroValue::String { value } => Self::String(value.into()),
            LoroValue::List { value } => {
                Self::List(value.into_iter().map(Into::into).collect())
            }
            LoroValue::Map { value } => {
                Self::Map(value.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
            LoroValue::Container { value } => Self::Container(value.into()),
        }
    }
}

impl From<&LoroValue> for loro::LoroValue {
    fn from(value: &LoroValue) -> Self {
        match value {
            LoroValue::Null => Self::Null,
            LoroValue::Bool { value } => Self::Bool(*value),
            LoroValue::Double { value } => Self::Double(*value),
            LoroValue::I64 { value } => Self::I64(*value),
            LoroValue::Binary { value } => Self::Binary(value.clone().into()),
            LoroValue::String { value } => Self::String(value.clone().into()),
            LoroValue::List { value } => {
                Self::List(value.iter().map(Into::into).collect())
            }
            LoroValue::Map { value } => {
                Self::Map(value.iter().map(|(k, v)| (k.clone(), v.into())).collect())
            }
            LoroValue::Container { value } => Self::Container(value.into()),
        }
    }
}

impl From<loro::LoroValue> for LoroValue {
    fn from(value: loro::LoroValue) -> Self {
        match value {
            loro::LoroValue::Null => Self::Null,
            loro::LoroValue::Bool(value) => Self::Bool { value },
            loro::LoroValue::Double(value) => Self::Double { value },
            loro::LoroValue::I64(value) => Self::I64 { value },
            loro::LoroValue::Binary(value) => Self::Binary {
                value: value.to_vec(),
            },
            loro::LoroValue::String(value) => Self::String {
                value: value.to_string(),
            },
            loro::LoroValue::List(value) => Self::List {
                value: (*value).clone().into_iter().map(Into::into).collect(),
            },
            loro::LoroValue::Map(value) => Self::Map {
                value: (*value)
                    .clone()
                    .into_iter()
                    .map(|(k, v)| (k, v.into()))
                    .collect(),
            },
            loro::LoroValue::Container(value) => Self::Container {
                value: value.into(),
            },
        }
    }
}

impl From<ContainerType> for loro::ContainerType {
    fn from(value: ContainerType) -> Self {
        match value {
            ContainerType::Text => Self::Text,
            ContainerType::Map => Self::Map,
            ContainerType::List => Self::List,
            ContainerType::MovableList => Self::MovableList,
            ContainerType::Tree => Self::Tree,
            ContainerType::Counter => Self::Counter,
            ContainerType::Unknown { kind } => Self::Unknown(kind),
        }
    }
}

impl From<loro::ContainerType> for ContainerType {
    fn from(value: loro::ContainerType) -> Self {
        match value {
            loro::ContainerType::Text => Self::Text,
            loro::ContainerType::Map => Self::Map,
            loro::ContainerType::List => Self::List,
            loro::ContainerType::MovableList => Self::MovableList,
            loro::ContainerType::Tree => Self::Tree,
            loro::ContainerType::Counter => Self::Counter,
            loro::ContainerType::Unknown(kind) => Self::Unknown { kind },
        }
    }
}

impl From<ContainerID> for loro::ContainerID {
    fn from(value: ContainerID) -> Self {
        match value {
            ContainerID::Root {
                name,
                container_type,
            } => Self::Root {
                name: name.into(),
                container_type: container_type.into(),
            },
            ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => Self::Normal {
                peer,
                counter,
                container_type: container_type.into(),
            },
        }
    }
}

impl From<&ContainerID> for loro::ContainerID {
    fn from(value: &ContainerID) -> Self {
        match value {
            ContainerID::Root {
                name,
                container_type,
            } => Self::Root {
                name: name.clone().into(),
                container_type: (*container_type).into(),
            },
            ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => Self::Normal {
                peer: *peer,
                counter: *counter,
                container_type: (*container_type).into(),
            },
        }
    }
}

impl From<loro::ContainerID> for ContainerID {
    fn from(value: loro::ContainerID) -> Self {
        match value {
            loro::ContainerID::Root {
                name,
                container_type,
            } => Self::Root {
                name: name.to_string(),
                container_type: container_type.into(),
            },
            loro::ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => Self::Normal {
                peer,
                counter,
                container_type: container_type.into(),
            },
        }
    }
}

impl From<&loro::ContainerID> for ContainerID {
    fn from(value: &loro::ContainerID) -> Self {
        match value {
            loro::ContainerID::Root {
                name,
                container_type,
            } => Self::Root {
                name: name.to_string(),
                container_type: (*container_type).into(),
            },
            loro::ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => Self::Normal {
                peer: *peer,
                counter: *counter,
                container_type: (*container_type).into(),
            },
        }
    }
}
