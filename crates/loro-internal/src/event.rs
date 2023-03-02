use enum_as_inner::EnumAsInner;
use serde::{Deserialize, Serialize};

use crate::{
    container::ContainerID, delta::{SeqDelta, MapDiff}, version::Frontiers, InternalString, LoroValue,
};

#[derive(Debug)]
pub struct RawEvent {
    pub container_id: ContainerID,
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub local: bool,
    pub diff: Vec<Diff>,
    pub abs_path: Path,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub current_target: Option<ContainerID>,
    pub target: ContainerID,
    /// the relative path from current_target to target
    pub relative_path: Path,
    pub absolute_path: Path,
    pub diff: Vec<Diff>,
    pub local: bool,
}

#[derive(Debug)]
pub(crate) struct PathAndTarget {
    pub relative_path: Path,
    pub target: Option<ContainerID>,
}

#[derive(Debug, Default)]
pub(crate) struct EventDispatch {
    pub sub_ids: Vec<SubscriptionID>,
    pub rewrite: Option<PathAndTarget>,
}

pub type Path = Vec<Index>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
}

#[derive(Clone, Debug, EnumAsInner, Serialize)]
pub enum Diff {
    List(SeqDelta<Vec<LoroValue>>),
    Text(SeqDelta<String>),
    Map(MapDiff),
}

// pub type Observer = Box<dyn FnMut(&Event) + Send>;
#[derive(Default)]
pub(crate) struct ObserverOptions {
    pub(crate) once: bool,
    pub(crate) container: Option<ContainerID>,
    pub(crate) deep: bool,
}

impl ObserverOptions {
    fn with_container(mut self, container: ContainerID) -> Self {
        self.container.replace(container);
        self
    }
}

pub type ObserverHandler = Box<dyn FnMut(&Event) + Send>;

pub struct Observer {
    handler: ObserverHandler,
    options: ObserverOptions,
}

impl Observer {
    pub fn new(
        handler: ObserverHandler,
        container: Option<ContainerID>,
        once: bool,
        deep: bool,
    ) -> Self {
        let options = ObserverOptions {
            container,
            once,
            deep,
        };
        Self { handler, options }
    }

    pub fn new_root(handler: ObserverHandler) -> Self {
        Self {
            handler,
            options: ObserverOptions::default(),
        }
    }

    pub fn new_container(handler: ObserverHandler, container: ContainerID) -> Self {
        Self {
            handler,
            options: ObserverOptions::default().with_container(container),
        }
    }

    pub fn container(&self) -> &Option<ContainerID> {
        &self.options.container
    }

    pub fn root(&self) -> bool {
        self.options.container.is_none()
    }

    pub fn deep(&self) -> bool {
        self.options.deep
    }

    pub fn with_once(mut self, once: bool) -> Self {
        self.options.once = once;
        self
    }

    pub fn with_deep(mut self, deep: bool) -> Self {
        self.options.deep = deep;
        self
    }

    pub fn once(&self) -> bool {
        self.options.once
    }

    pub fn call(&mut self, event: &Event) {
        (self.handler)(event)
    }
}

pub type SubscriptionID = u32;
