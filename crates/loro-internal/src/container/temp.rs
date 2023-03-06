use enum_as_inner::EnumAsInner;
use fxhash::FxHashSet;

use crate::{ContainerType, InternalString};

use super::{
    checker::{ListChecker, MapChecker, TextChecker},
    registry::ContainerIdx,
};

#[derive(Debug, Clone, EnumAsInner)]
pub enum ContainerTemp {
    List(ListTemp),
    Map(MapTemp),
    Text(TextTemp),
}

impl ContainerTemp {
    pub(crate) fn new(idx: ContainerIdx, type_: ContainerType) -> Self {
        match type_ {
            ContainerType::List => Self::List(ListTemp::from_idx(idx)),
            ContainerType::Map => Self::Map(MapTemp::from_idx(idx)),
            ContainerType::Text => Self::Text(TextTemp::from_idx(idx)),
        }
    }

    pub fn idx(&self) -> ContainerIdx {
        match self {
            ContainerTemp::List(x) => x.idx,
            ContainerTemp::Map(x) => x.idx,
            ContainerTemp::Text(x) => x.idx,
        }
    }

    pub fn type_(&self) -> ContainerType {
        match self {
            ContainerTemp::List(_) => ContainerType::List,
            ContainerTemp::Map(_) => ContainerType::Map,
            ContainerTemp::Text(_) => ContainerType::Text,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListTemp {
    idx: ContainerIdx,
    checker: ListChecker,
}

#[derive(Debug, Clone)]
pub struct MapTemp {
    idx: ContainerIdx,
    checker: MapChecker,
}

#[derive(Debug, Clone)]
pub struct TextTemp {
    idx: ContainerIdx,
    checker: TextChecker,
}

impl ListTemp {
    fn from_idx(idx: ContainerIdx) -> Self {
        ListTemp {
            idx,
            checker: ListChecker::from_idx(idx),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.checker.current_length
    }
}

impl MapTemp {
    fn from_idx(idx: ContainerIdx) -> Self {
        MapTemp {
            idx,
            checker: MapChecker::from_idx(idx),
        }
    }
    pub(crate) fn keys(&self) -> Vec<InternalString> {
        self.checker.keys.iter().cloned().collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.checker.keys.len()
    }
}

impl TextTemp {
    fn from_idx(idx: ContainerIdx) -> Self {
        TextTemp {
            idx,
            checker: TextChecker::from_idx(idx),
        }
    }
    pub(crate) fn len(&self) -> usize {
        self.checker.current_length
    }
}
