use std::{cmp::Ordering, ops::Deref, sync::Arc};

use loro::{
    cursor::{CannotFindRelativePosition, Cursor, PosQueryResult},
    CommitOptions, Frontiers, FrontiersNotIncluded, JsonSchema, LoroDoc as InnerLoroDoc, LoroError,
    SubID, VersionVector,
};

use crate::{
    event::{DiffEvent, Subscriber},
    ContainerID, ContainerIdLike, Index, LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText,
    LoroTree, LoroValue, ValueOrContainer,
};

pub struct LoroDoc {
    doc: InnerLoroDoc,
}

impl LoroDoc {
    pub fn new() -> Self {
        Self {
            doc: InnerLoroDoc::new(),
        }
    }

    pub fn fork(&self) -> Arc<Self> {
        let doc = self.doc.fork();
        Arc::new(LoroDoc { doc })
    }

    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        self.doc.cmp_frontiers(a, b)
    }

    pub fn get_movable_list(&self, id: Arc<dyn ContainerIdLike>) -> Arc<LoroMovableList> {
        Arc::new(LoroMovableList {
            list: self.doc.get_movable_list(loro::ContainerID::from(
                id.as_container_id(crate::ContainerType::MovableList),
            )),
        })
    }

    pub fn get_list(&self, id: Arc<dyn ContainerIdLike>) -> Arc<LoroList> {
        Arc::new(LoroList {
            list: self.doc.get_list(loro::ContainerID::from(
                id.as_container_id(crate::ContainerType::List),
            )),
        })
    }

    pub fn get_map(&self, id: Arc<dyn ContainerIdLike>) -> Arc<LoroMap> {
        Arc::new(LoroMap {
            map: self.doc.get_map(loro::ContainerID::from(
                id.as_container_id(crate::ContainerType::Map),
            )),
        })
    }

    pub fn get_text(&self, id: Arc<dyn ContainerIdLike>) -> Arc<LoroText> {
        Arc::new(LoroText {
            text: self.doc.get_text(loro::ContainerID::from(
                id.as_container_id(crate::ContainerType::Text),
            )),
        })
    }

    pub fn get_tree(&self, id: Arc<dyn ContainerIdLike>) -> Arc<LoroTree> {
        Arc::new(LoroTree {
            tree: self.doc.get_tree(loro::ContainerID::from(
                id.as_container_id(crate::ContainerType::Tree),
            )),
        })
    }

    pub fn get_counter(&self, id: Arc<dyn ContainerIdLike>) -> Arc<LoroCounter> {
        Arc::new(LoroCounter {
            counter: self.doc.get_counter(loro::ContainerID::from(
                id.as_container_id(crate::ContainerType::Counter),
            )),
        })
    }

    pub fn commit_with(&self, options: CommitOptions) {
        self.doc.commit_with(options)
    }

    pub fn import_json_updates<T: TryInto<JsonSchema>>(&self, json: T) -> Result<(), LoroError> {
        self.doc.import_json_updates(json)
    }

    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<Arc<VersionVector>> {
        self.doc.frontiers_to_vv(frontiers).map(Arc::new)
    }

    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Arc<Frontiers> {
        Arc::new(self.doc.vv_to_frontiers(vv))
    }

    pub fn oplog_vv(&self) -> Arc<VersionVector> {
        Arc::new(self.doc.oplog_vv())
    }

    pub fn state_vv(&self) -> Arc<VersionVector> {
        Arc::new(self.doc.state_vv())
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.doc.get_deep_value().into()
    }

    pub fn oplog_frontiers(&self) -> Arc<Frontiers> {
        Arc::new(self.doc.oplog_frontiers())
    }

    pub fn state_frontiers(&self) -> Arc<Frontiers> {
        Arc::new(self.doc.state_frontiers())
    }
    pub fn subscribe(&self, container_id: &ContainerID, subscriber: Arc<dyn Subscriber>) -> SubID {
        self.doc.subscribe(
            &(container_id.into()),
            Arc::new(move |e| {
                subscriber.on_diff(DiffEvent::from(e));
            }),
        )
    }

    pub fn subscribe_root(&self, subscriber: Arc<dyn Subscriber>) -> SubID {
        // self.doc.subscribe_root(callback)
        self.doc.subscribe_root(Arc::new(move |e| {
            subscriber.on_diff(DiffEvent::from(e));
        }))
    }
    pub fn get_by_path(&self, path: &[Index]) -> Option<Arc<dyn ValueOrContainer>> {
        self.doc
            .get_by_path(&path.iter().map(|v| v.clone().into()).collect::<Vec<_>>())
            .map(|x| Arc::new(x) as Arc<dyn ValueOrContainer>)
    }

    pub fn get_by_str_path(&self, path: &str) -> Option<Arc<dyn ValueOrContainer>> {
        self.doc
            .get_by_str_path(path)
            .map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>)
    }

    pub fn get_cursor_pos(
        &self,
        cursor: &Cursor,
    ) -> Result<PosQueryResult, CannotFindRelativePosition> {
        self.doc.get_cursor_pos(cursor)
    }

    pub fn len_ops(&self) -> u64 {
        self.doc.len_ops() as u64
    }

    pub fn len_changes(&self) -> u64 {
        self.doc.len_changes() as u64
    }
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for LoroDoc {
    type Target = InnerLoroDoc;
    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}
