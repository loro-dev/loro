use std::{collections::HashMap, sync::Arc};

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
    fn from(value: LoroValue) -> loro::LoroValue {
        match value {
            LoroValue::Null => loro::LoroValue::Null,
            LoroValue::Bool { value } => loro::LoroValue::Bool(value),
            LoroValue::Double { value } => loro::LoroValue::Double(value),
            LoroValue::I64 { value } => loro::LoroValue::I64(value),
            LoroValue::Binary { value } => loro::LoroValue::Binary(Arc::new(value)),
            LoroValue::String { value } => loro::LoroValue::String(Arc::new(value)),
            LoroValue::List { value } => {
                loro::LoroValue::List(Arc::new(value.into_iter().map(Into::into).collect()))
            }
            LoroValue::Map { value } => loro::LoroValue::Map(Arc::new(
                value.into_iter().map(|(k, v)| (k, v.into())).collect(),
            )),
            LoroValue::Container { value } => loro::LoroValue::Container(value.into()),
        }
    }
}

impl<'a> From<&'a LoroValue> for loro::LoroValue {
    fn from(value: &LoroValue) -> loro::LoroValue {
        match value {
            LoroValue::Null => loro::LoroValue::Null,
            LoroValue::Bool { value } => loro::LoroValue::Bool(*value),
            LoroValue::Double { value } => loro::LoroValue::Double(*value),
            LoroValue::I64 { value } => loro::LoroValue::I64(*value),
            LoroValue::Binary { value } => loro::LoroValue::Binary(Arc::new(value.clone())),
            LoroValue::String { value } => loro::LoroValue::String(Arc::new(value.clone())),
            LoroValue::List { value } => {
                loro::LoroValue::List(Arc::new(value.into_iter().map(Into::into).collect()))
            }
            LoroValue::Map { value } => loro::LoroValue::Map(Arc::new(
                value.iter().map(|(k, v)| (k.clone(), v.into())).collect(),
            )),
            LoroValue::Container { value } => loro::LoroValue::Container(value.into()),
        }
    }
}

impl From<loro::LoroValue> for LoroValue {
    fn from(value: loro::LoroValue) -> LoroValue {
        match value {
            loro::LoroValue::Null => LoroValue::Null,
            loro::LoroValue::Bool(value) => LoroValue::Bool { value },
            loro::LoroValue::Double(value) => LoroValue::Double { value },
            loro::LoroValue::I64(value) => LoroValue::I64 { value },
            loro::LoroValue::Binary(value) => LoroValue::Binary {
                value: value.to_vec(),
            },
            loro::LoroValue::String(value) => LoroValue::String {
                value: value.to_string(),
            },
            loro::LoroValue::List(value) => LoroValue::List {
                value: (*value).clone().into_iter().map(Into::into).collect(),
            },
            loro::LoroValue::Map(value) => LoroValue::Map {
                value: (*value)
                    .clone()
                    .into_iter()
                    .map(|(k, v)| (k, v.into()))
                    .collect(),
            },
            loro::LoroValue::Container(value) => LoroValue::Container {
                value: value.into(),
            },
        }
    }
}

impl From<ContainerType> for loro::ContainerType {
    fn from(value: ContainerType) -> loro::ContainerType {
        match value {
            ContainerType::Text => loro::ContainerType::Text,
            ContainerType::Map => loro::ContainerType::Map,
            ContainerType::List => loro::ContainerType::List,
            ContainerType::MovableList => loro::ContainerType::MovableList,
            ContainerType::Tree => loro::ContainerType::Tree,
            ContainerType::Counter => loro::ContainerType::Counter,
            ContainerType::Unknown { kind } => loro::ContainerType::Unknown(kind),
        }
    }
}

impl From<loro::ContainerType> for ContainerType {
    fn from(value: loro::ContainerType) -> ContainerType {
        match value {
            loro::ContainerType::Text => ContainerType::Text,
            loro::ContainerType::Map => ContainerType::Map,
            loro::ContainerType::List => ContainerType::List,
            loro::ContainerType::MovableList => ContainerType::MovableList,
            loro::ContainerType::Tree => ContainerType::Tree,
            loro::ContainerType::Counter => ContainerType::Counter,
            loro::ContainerType::Unknown(kind) => ContainerType::Unknown { kind },
        }
    }
}

impl From<ContainerID> for loro::ContainerID {
    fn from(value: ContainerID) -> loro::ContainerID {
        match value {
            ContainerID::Root {
                name,
                container_type,
            } => loro::ContainerID::Root {
                name: name.into(),
                container_type: container_type.into(),
            },
            ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => loro::ContainerID::Normal {
                peer,
                counter,
                container_type: container_type.into(),
            },
        }
    }
}

impl<'a> From<&'a ContainerID> for loro::ContainerID {
    fn from(value: &ContainerID) -> loro::ContainerID {
        match value {
            ContainerID::Root {
                name,
                container_type,
            } => loro::ContainerID::Root {
                name: name.clone().into(),
                container_type: (*container_type).into(),
            },
            ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => loro::ContainerID::Normal {
                peer: *peer,
                counter: *counter,
                container_type: (*container_type).into(),
            },
        }
    }
}

impl From<loro::ContainerID> for ContainerID {
    fn from(value: loro::ContainerID) -> ContainerID {
        match value {
            loro::ContainerID::Root {
                name,
                container_type,
            } => ContainerID::Root {
                name: name.to_string(),
                container_type: container_type.into(),
            },
            loro::ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => ContainerID::Normal {
                peer,
                counter,
                container_type: container_type.into(),
            },
        }
    }
}

impl<'a> From<&'a loro::ContainerID> for ContainerID {
    fn from(value: &loro::ContainerID) -> ContainerID {
        match value {
            loro::ContainerID::Root {
                name,
                container_type,
            } => ContainerID::Root {
                name: name.to_string(),
                container_type: (*container_type).into(),
            },
            loro::ContainerID::Normal {
                peer,
                counter,
                container_type,
            } => ContainerID::Normal {
                peer: *peer,
                counter: *counter,
                container_type: (*container_type).into(),
            },
        }
    }
}
