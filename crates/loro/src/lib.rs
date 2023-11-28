pub use loro_internal::container::{ContainerID, ContainerType};
pub use loro_internal::version::Frontiers;
pub use loro_internal::{handler::Handler, ListHandler, MapHandler, TextHandler, TreeHandler};
pub use loro_internal::{LoroError, LoroResult, LoroValue};

use loro_internal::container::IntoContainerId;
use loro_internal::{LoroDoc as InnerLoroDoc, VersionVector};
use std::cmp::Ordering;

/// `LoroDoc` is the entry for the whole document.
/// When it's dropped, all the associated `Container`s will be invalidated.
pub struct LoroDoc {
    doc: InnerLoroDoc,
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl LoroDoc {
    pub fn new() -> Self {
        let mut doc = InnerLoroDoc::default();
        doc.start_auto_commit();
        LoroDoc { doc }
    }

    pub fn attach(&mut self) {
        self.doc.attach()
    }

    pub fn checkout(&mut self, frontiers: &Frontiers) -> LoroResult<()> {
        self.doc.checkout(frontiers)
    }

    pub fn cmp_frontiers(&self, other: &Frontiers) -> Ordering {
        self.doc.cmp_frontiers(other)
    }

    pub fn detach(&mut self) {
        self.doc.detach()
    }

    pub fn import_batch(&mut self, bytes: &[Vec<u8>]) -> LoroResult<()> {
        self.doc.import_batch(bytes)
    }

    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        self.doc.get_list(id)
    }

    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        self.doc.get_map(id)
    }

    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        self.doc.get_text(id)
    }

    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        self.doc.get_tree(id)
    }

    pub fn commit(&self) {
        self.doc.commit_then_renew()
    }

    pub fn is_detached(&self) -> bool {
        self.doc.is_detached()
    }

    pub fn import(&self, bytes: &[u8]) -> Result<(), LoroError> {
        self.doc.import_with(bytes, "".into())
    }

    pub fn import_with(&self, bytes: &[u8], origin: &str) -> Result<(), LoroError> {
        self.doc.import_with(bytes, origin.into())
    }

    /// Export all the ops not included in the given `VersionVector`
    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        self.doc.export_from(vv)
    }

    pub fn export_snapshot(&self) -> Vec<u8> {
        self.doc.export_snapshot()
    }

    /// Convert `Frontiers` into `VersionVector`
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        self.doc.frontiers_to_vv(frontiers)
    }

    /// Convert `VersionVector` into `Frontiers`
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        self.doc.vv_to_frontiers(vv)
    }

    /// Get the `VersionVector` version of `OpLog`
    pub fn oplog_vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Get the `VersionVector` version of `OpLog`
    pub fn state_vv(&self) -> VersionVector {
        self.doc.state_vv()
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.doc.get_deep_value()
    }

    /// Get the `Frontiers` version of `OpLog`
    pub fn oplog_frontiers(&self) -> Frontiers {
        self.doc.oplog_frontiers()
    }

    /// Get the `Frontiers` version of `DocState`
    ///
    /// [Learn more about `Frontiers`]()
    pub fn state_frontiers(&self) -> Frontiers {
        self.doc.state_frontiers()
    }
}
