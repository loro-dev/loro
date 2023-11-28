use loro_internal::container::richtext::TextStyleInfoFlag;
pub use loro_internal::container::{ContainerID, ContainerType};
use loro_internal::handler::TextDelta;
pub use loro_internal::handler::ValueOrContainer;
pub use loro_internal::version::Frontiers;
pub use loro_internal::{LoroError, LoroResult, LoroValue};

use loro_internal::container::IntoContainerId;
use loro_internal::{
    handler::Handler as InnerHandler, ListHandler as InnerListHandler,
    MapHandler as InnerMapHandler, TextHandler as InnerTextHandler,
    TreeHandler as InnerTreeHandler,
};
use loro_internal::{LoroDoc as InnerLoroDoc, VersionVector};
use std::cmp::Ordering;

/// `LoroDoc` is the entry for the whole document.
/// When it's dropped, all the associated [`Handler`]s will be invalidated.
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
        ListHandler {
            handler: self.doc.get_list(id),
        }
    }

    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        MapHandler {
            handler: self.doc.get_map(id),
        }
    }

    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        TextHandler {
            handler: self.doc.get_text(id),
        }
    }

    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        TreeHandler {
            handler: self.doc.get_tree(id),
        }
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

#[derive(Clone, Debug)]
pub struct ListHandler {
    handler: InnerListHandler,
}

impl ListHandler {
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(pos, v)
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    pub fn get(&self, index: usize) -> Option<ValueOrContainer> {
        self.handler.get_(index)
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    pub fn id(&self) -> ContainerID {
        self.handler.id()
    }

    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.handler.pop()
    }

    pub fn push(&self, v: LoroValue) -> LoroResult<()> {
        self.handler.push(v)
    }

    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut(ValueOrContainer),
    {
        self.handler.for_each(f)
    }

    pub fn len(&self) -> usize {
        self.handler.len()
    }

    pub fn insert_container(&self, pos: usize, c_type: ContainerType) -> LoroResult<Handler> {
        Ok(Handler::from(self.handler.insert_container(pos, c_type)?))
    }
}

#[derive(Clone, Debug)]
pub struct MapHandler {
    handler: InnerMapHandler,
}

impl MapHandler {
    pub fn delete(&self, key: &str) -> LoroResult<()> {
        self.handler.delete(key)
    }

    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut(&str, ValueOrContainer),
    {
        self.handler.for_each(f)
    }

    pub fn insert(&self, key: &str, value: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(key, value)
    }

    pub fn len(&self) -> usize {
        self.handler.len()
    }

    pub fn id(&self) -> ContainerID {
        self.handler.id()
    }

    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<ValueOrContainer> {
        self.handler.get_(key)
    }

    pub fn insert_container(&self, key: &str, c_type: ContainerType) -> LoroResult<Handler> {
        Ok(Handler::from(self.handler.insert_container(key, c_type)?))
    }
}

#[derive(Clone, Debug)]
pub struct TextHandler {
    handler: InnerTextHandler,
}

impl TextHandler {
    pub fn insert(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.handler.insert(pos, s)
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    pub fn len_utf8(&self) -> usize {
        self.handler.len_utf8()
    }

    pub fn id(&self) -> ContainerID {
        self.handler.id()
    }

    pub fn apply_delta(&self, delta: &[TextDelta]) -> LoroResult<()> {
        self.handler.apply_delta(delta)
    }

    pub fn mark(
        &self,
        start: usize,
        end: usize,
        key: &str,
        value: LoroValue,
        flag: TextStyleInfoFlag,
    ) -> LoroResult<()> {
        self.handler.mark(start, end, key, value, flag)
    }

    pub fn get_richtext_value(&self) -> LoroValue {
        self.handler.get_richtext_value()
    }

    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.handler.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct TreeHandler {
    handler: InnerTreeHandler,
}

use enum_as_inner::EnumAsInner;

#[derive(Clone, Debug, EnumAsInner)]
pub enum Handler {
    List(ListHandler),
    Map(MapHandler),
    Text(TextHandler),
    Tree(TreeHandler),
}

impl From<InnerHandler> for Handler {
    fn from(value: InnerHandler) -> Self {
        match value {
            InnerHandler::Text(x) => Handler::Text(TextHandler { handler: x }),
            InnerHandler::Map(x) => Handler::Map(MapHandler { handler: x }),
            InnerHandler::List(x) => Handler::List(ListHandler { handler: x }),
            InnerHandler::Tree(x) => Handler::Tree(TreeHandler { handler: x }),
        }
    }
}
