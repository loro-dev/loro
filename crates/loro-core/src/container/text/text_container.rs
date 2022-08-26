use std::{pin::Pin, ptr::NonNull, rc::Weak};

use fxhash::FxHashMap;
use rle::RleVec;
use serde::Serialize;
use smallvec::SmallVec;

use crate::{
    change::Change,
    container::{Container, ContainerID, ContainerType},
    id::{Counter, ID},
    op::{utils::downcast_ref, Op},
    op::{OpContent, OpProxy},
    span::IdSpan,
    value::{InsertValue, LoroValue},
    version::TotalOrderStamp,
    ClientID, InternalString, Lamport, LogStore, OpType,
};

use super::y_span::YSpan;

#[derive(Clone, Debug)]
struct DagNode {
    id: IdSpan,
    deps: SmallVec<[ID; 2]>,
}

#[derive(Clone, Debug)]
pub struct TextContainer {
    id: ContainerID,
    sub_dag: FxHashMap<ClientID, RleVec<DagNode, ()>>,
    log_store: NonNull<LogStore>,
}

impl TextContainer {
    pub fn insert(&mut self, pos: usize, text: &str) -> ID {
        todo!()
    }

    pub fn delete(&mut self, pos: usize, len: usize) {}
}

impl Container for TextContainer {
    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn type_(&self) -> ContainerType {
        ContainerType::Text
    }

    fn apply(&mut self, op: &OpProxy) {
        todo!()
    }

    fn checkout_version(&mut self, vv: &crate::VersionVector) {
        todo!()
    }

    fn get_value(&mut self) -> &LoroValue {
        todo!()
    }
}
