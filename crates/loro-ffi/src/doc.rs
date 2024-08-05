use std::{cmp::Ordering, ops::Deref, sync::Arc};

use loro::{
    cursor::{CannotFindRelativePosition, Cursor, PosQueryResult},
    event::{DiffEvent, Subscriber},
    CommitOptions, Frontiers, FrontiersNotIncluded, Index, IntoContainerId, JsonSchema,
    LoroCounter, LoroDoc as InnerLoroDoc, LoroError, LoroList, LoroMap, LoroMovableList, LoroText,
    LoroTree, SubID, ValueOrContainer, VersionVector,
};

use crate::{ContainerID, LoroValue};

pub struct LoroDoc {
    doc: InnerLoroDoc,
}

impl LoroDoc {
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

    pub fn get_movable_list<I: IntoContainerId>(&self, id: I) -> Arc<LoroMovableList> {
        Arc::new(self.doc.get_movable_list(id))
    }

    pub fn get_list<I: IntoContainerId>(&self, id: I) -> Arc<LoroList> {
        Arc::new(self.doc.get_list(id))
    }

    pub fn get_map<I: IntoContainerId>(&self, id: I) -> Arc<LoroMap> {
        Arc::new(self.doc.get_map(id))
    }

    pub fn get_text<I: IntoContainerId>(&self, id: I) -> Arc<LoroText> {
        Arc::new(self.doc.get_text(id))
    }

    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> Arc<LoroTree> {
        Arc::new(self.doc.get_tree(id))
    }

    pub fn get_counter<I: IntoContainerId>(&self, id: I) -> Arc<LoroCounter> {
        Arc::new(self.doc.get_counter(id))
    }

    pub fn commit_with(&self, options: CommitOptions) {
        self.doc.commit_with(options)
    }

    pub fn import_json_updates<T: TryInto<JsonSchema>>(&self, json: T) -> Result<(), LoroError> {
        self.doc.import_json_updates(json)
    }

    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        self.doc.frontiers_to_vv(frontiers)
    }

    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        self.doc.vv_to_frontiers(vv)
    }

    pub fn oplog_vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    pub fn state_vv(&self) -> VersionVector {
        self.doc.state_vv()
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.doc.get_deep_value().into()
    }

    pub fn oplog_frontiers(&self) -> Frontiers {
        self.doc.oplog_frontiers()
    }

    pub fn state_frontiers(&self) -> Frontiers {
        self.doc.state_frontiers()
    }
    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> SubID {
        self.doc.subscribe(
            &(container_id.into()),
            Arc::new(move |e| {
                callback(DiffEvent::from(e));
            }),
        )
    }

    pub fn subscribe_root(&self, callback: Subscriber) -> SubID {
        // self.doc.subscribe_root(callback)
        self.doc.subscribe_root(Arc::new(move |e| {
            callback(DiffEvent::from(e));
        }))
    }
    pub fn get_by_path(&self, path: &[Index]) -> Option<ValueOrContainer> {
        self.doc.get_by_path(path)
    }

    pub fn get_by_str_path(&self, path: &str) -> Option<ValueOrContainer> {
        self.doc.get_by_str_path(path)
    }

    pub fn get_cursor_pos(
        &self,
        cursor: &Cursor,
    ) -> Result<PosQueryResult, CannotFindRelativePosition> {
        self.doc.get_cursor_pos(cursor)
    }
}

impl Deref for LoroDoc {
    type Target = InnerLoroDoc;
    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}
