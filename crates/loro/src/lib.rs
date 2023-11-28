#![doc = include_str!("../README.md")]
use either::Either;
use loro_internal::container::richtext::TextStyleInfoFlag;
use loro_internal::container::IntoContainerId;
use loro_internal::handler::TextDelta;
use loro_internal::handler::ValueOrContainer;
use loro_internal::id::PeerID;
use loro_internal::id::TreeID;
use loro_internal::LoroDoc as InnerLoroDoc;
use loro_internal::{
    handler::Handler as InnerHandler, ListHandler as InnerListHandler,
    MapHandler as InnerMapHandler, TextHandler as InnerTextHandler,
    TreeHandler as InnerTreeHandler,
};
use std::cmp::Ordering;
use std::ops::Range;

pub use loro_internal::container::richtext::ExpandType;
pub use loro_internal::container::{ContainerID, ContainerType};
pub use loro_internal::version::{Frontiers, VersionVector};
pub use loro_internal::{LoroError, LoroResult, LoroValue, ToJson};

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

    /// Attach the document state to the latest known version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    pub fn attach(&mut self) {
        self.doc.attach()
    }

    /// Checkout the `DocState` to a specific version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// You should call `attach` to attach the `DocState` to the lastest version of `OpLog`.
    pub fn checkout(&mut self, frontiers: &Frontiers) -> LoroResult<()> {
        self.doc.checkout(frontiers)
    }

    pub fn cmp_frontiers(&self, other: &Frontiers) -> Ordering {
        self.doc.cmp_frontiers(other)
    }

    /// Force the document enter the detached mode.
    ///
    /// In this mode, when you importing new updates, the [loro_internal::DocState] will not be changed.
    ///
    /// Learn more at https://loro.dev/docs/advanced/doc_state_and_oplog#attacheddetached-status
    pub fn detach(&mut self) {
        self.doc.detach()
    }

    /// Import a batch of updates/snapshot.
    ///
    /// The data can be in arbitrary order. The import result will be the same.
    pub fn import_batch(&mut self, bytes: &[Vec<u8>]) -> LoroResult<()> {
        self.doc.import_batch(bytes)
    }

    /// Get a [ListHandler] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> LoroList {
        LoroList {
            handler: self.doc.get_list(id),
        }
    }

    /// Get a [MapHandler] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> LoroMap {
        LoroMap {
            handler: self.doc.get_map(id),
        }
    }

    /// Get a [TextHandler] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> LoroText {
        LoroText {
            handler: self.doc.get_text(id),
        }
    }

    /// Get a [TreeHandler] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> LoroTree {
        LoroTree {
            handler: self.doc.get_tree(id),
        }
    }

    /// Commit the cumulative auto commit transaction.
    ///
    /// There is a transaction behind every operation.
    /// It will automatically commit when users invoke export or import.
    /// The event will be sent after a transaction is committed
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

    #[cfg(feature = "test_utils")]
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        self.doc.set_peer_id(peer)
    }
}

/// LoroList container. It's used to model array.
///
/// It can have sub containers.
///
/// ```
/// # use loro::{LoroDoc, ContainerType, ToJson};
/// # use serde_json::json;
/// let doc = LoroDoc::new();
/// let list = doc.get_list("list");
/// list.insert(0, 123).unwrap();
/// list.insert(1, "h").unwrap();
/// assert_eq!(
///     doc.get_deep_value().to_json_value(),
///     json!({
///         "list": [123, "h"]
///     })
/// );
/// ```
#[derive(Clone, Debug)]
pub struct LoroList {
    handler: InnerListHandler,
}

impl LoroList {
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(pos, v)
    }

    #[inline]
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<Either<LoroValue, Container>> {
        match self.handler.get_(index) {
            Some(ValueOrContainer::Container(c)) => Some(Either::Right(c.into())),
            Some(ValueOrContainer::Value(v)) => Some(Either::Left(v)),
            None => None,
        }
    }

    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    #[inline]
    pub fn id(&self) -> ContainerID {
        self.handler.id()
    }

    #[inline]
    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.handler.pop()
    }

    #[inline]
    pub fn push(&self, v: LoroValue) -> LoroResult<()> {
        self.handler.push(v)
    }

    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut(ValueOrContainer),
    {
        self.handler.for_each(f)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.handler.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    /// Insert a container with the given type at the given index.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, ContainerType, ToJson};
    /// # use serde_json::json;
    /// let doc = LoroDoc::new();
    /// let list = doc.get_list("m");
    /// let text = list.insert_container(0, ContainerType::Text).unwrap().into_text().unwrap();
    /// text.insert(0, "12");
    /// text.insert(0, "0");
    /// assert_eq!(doc.get_deep_value().to_json_value(), json!({"m": ["012"]}));
    /// ```
    #[inline]
    pub fn insert_container(&self, pos: usize, c_type: ContainerType) -> LoroResult<Container> {
        Ok(Container::from(self.handler.insert_container(pos, c_type)?))
    }
}

/// LoroMap container.
///
/// It's LWW(Last-Write-Win) Map. It can support Multi-Value Map in the future.
///
/// # Example
/// ```
/// # use loro::{LoroDoc, ToJson, ExpandType, LoroValue};
/// # use serde_json::json;
/// let doc = LoroDoc::new();
/// let map = doc.get_map("map");
/// map.insert("key", "value").unwrap();
/// map.insert("true", true).unwrap();
/// map.insert("null", LoroValue::Null).unwrap();
/// map.insert("deleted", LoroValue::Null).unwrap();
/// map.delete("deleted").unwrap();
/// let text = map
///    .insert_container("text", loro_internal::ContainerType::Text).unwrap()
///    .into_text()
///    .unwrap();
/// text.insert(0, "Hello world!").unwrap();
/// assert_eq!(
///     doc.get_deep_value().to_json_value(),
///     json!({
///        "map": {
///            "key": "value",
///            "true": true,
///            "null": null,
///            "text": "Hello world!"
///        }
///    })
/// );
/// ```
#[derive(Clone, Debug)]
pub struct LoroMap {
    handler: InnerMapHandler,
}

impl LoroMap {
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

    pub fn get(&self, key: &str) -> Option<Either<LoroValue, Container>> {
        match self.handler.get_(key) {
            None => None,
            Some(ValueOrContainer::Container(c)) => Some(Either::Right(c.into())),
            Some(ValueOrContainer::Value(v)) => Some(Either::Left(v)),
        }
    }

    /// Insert a container with the given type at the given key.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, ContainerType, ToJson};
    /// # use serde_json::json;
    /// let doc = LoroDoc::new();
    /// let map = doc.get_map("m");
    /// let text = map.insert_container("t", ContainerType::Text).unwrap().into_text().unwrap();
    /// text.insert(0, "12");
    /// text.insert(0, "0");
    /// assert_eq!(doc.get_deep_value().to_json_value(), json!({"m": {"t": "012"}}));
    /// ```
    pub fn insert_container(&self, key: &str, c_type: ContainerType) -> LoroResult<Container> {
        Ok(Container::from(self.handler.insert_container(key, c_type)?))
    }
}

/// LoroText container. It's used to model plaintext/richtext.
#[derive(Clone, Debug)]
pub struct LoroText {
    handler: InnerTextHandler,
}

impl LoroText {
    /// Get the [ContainerID]  of the text container.
    pub fn id(&self) -> ContainerID {
        self.handler.id()
    }

    /// Insert a string at the given unicode position.
    pub fn insert(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.handler.insert(pos, s)
    }

    /// Delete a range of text at the given unicode position with unicode length.
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    pub fn len_utf8(&self) -> usize {
        self.handler.len_utf8()
    }

    pub fn len_unicode(&self) -> usize {
        self.handler.len_unicode()
    }

    pub fn len_utf16(&self) -> usize {
        self.handler.len_utf16()
    }

    /// Apply a [delta](https://quilljs.com/docs/delta/) to the text container.
    pub fn apply_delta(&self, delta: &[TextDelta]) -> LoroResult<()> {
        self.handler.apply_delta(delta)
    }

    /// Mark a range of text with a key-value pair.
    ///
    /// You can use it to create a highlight, make a range of text bold, or add a link to a range of text.
    ///
    /// You can specify the `expand` option to set the behavior when inserting text at the boundary of the range.
    ///
    /// - `after`(default): when inserting text right after the given range, the mark will be expanded to include the inserted text
    /// - `before`: when inserting text right before the given range, the mark will be expanded to include the inserted text
    /// - `none`: the mark will not be expanded to include the inserted text at the boundaries
    /// - `both`: when inserting text either right before or right after the given range, the mark will be expanded to include the inserted text
    ///
    /// *You should make sure that a key is always associated with the same expand type.*
    ///
    /// Note: this is not suitable for unmergeable annotations like comments.
    pub fn mark(
        &self,
        range: Range<usize>,
        expand: ExpandType,
        key: &str,
        value: impl Into<LoroValue>,
    ) -> LoroResult<()> {
        self.handler.mark(
            range.start,
            range.end,
            key,
            value.into(),
            TextStyleInfoFlag::new(true, expand, false, false),
        )
    }

    /// Unmark a range of text with a key and a value.
    ///
    /// You can use it to remove highlights, bolds or links
    ///
    /// You can specify the `expand` option to set the behavior when inserting text at the boundary of the range.
    ///
    /// **Note: You should specify the same expand type as when you mark the text.**
    ///
    /// - `after`(default): when inserting text right after the given range, the mark will be expanded to include the inserted text
    /// - `before`: when inserting text right before the given range, the mark will be expanded to include the inserted text
    /// - `none`: the mark will not be expanded to include the inserted text at the boundaries
    /// - `both`: when inserting text either right before or right after the given range, the mark will be expanded to include the inserted text
    ///
    /// *You should make sure that a key is always associated with the same expand type.*
    ///
    /// Note: you cannot delete unmergeable annotations like comments by this method.
    pub fn unmark(&self, range: Range<usize>, expand: ExpandType, key: &str) -> LoroResult<()> {
        let expand = expand.reverse();
        self.handler.mark(
            range.start,
            range.end,
            key,
            LoroValue::Null,
            TextStyleInfoFlag::new(true, expand, true, false),
        )
    }

    /// Get the text in [Delta](https://quilljs.com/docs/delta/) format.
    ///
    /// # Example
    /// ```
    /// # use loro::{LoroDoc, ToJson, ExpandType};
    /// # use serde_json::json;
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello world!").unwrap();
    /// text.mark(0..5, ExpandType::After, "bold", true).unwrap();
    /// assert_eq!(
    ///     text.to_delta().to_json_value(),
    ///     json!([
    ///         { "insert": "Hello", "attributes": {"bold": true} },
    ///         { "insert": " world!" },
    ///     ])
    /// );
    /// text.unmark(3..5, ExpandType::After, "bold").unwrap();
    /// assert_eq!(
    ///     text.to_delta().to_json_value(),
    ///     json!([
    ///         { "insert": "Hel", "attributes": {"bold": true} },
    ///         { "insert": "lo", "attributes": {"bold": null} },
    ///         { "insert": " world!" },
    ///    ])
    /// );
    /// ```
    pub fn to_delta(&self) -> LoroValue {
        self.handler.get_richtext_value()
    }

    /// Get the text content of the text container.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.handler.to_string()
    }
}

/// LoroTree container. It's used to model movable trees.
///
/// You may use it to model directories, outline or other movable hierarchical data.
#[derive(Clone, Debug)]
pub struct LoroTree {
    handler: InnerTreeHandler,
}

impl LoroTree {
    /// Create a new tree node and return the [`TreeID`].
    ///
    /// If the `parent` is `None`, the created node is the root of a tree.
    /// Otherwise, the created node is a child of the parent tree node.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// // create a root
    /// let root = tree.create(None).unwrap();
    /// // create a new child
    /// let child = tree.create(root).unwrap();
    /// ```
    pub fn create<T: Into<Option<TreeID>>>(&self, parent: T) -> LoroResult<TreeID> {
        self.handler.create(parent)
    }

    /// Move the `target` node to be a child of the `parent` node.
    ///
    /// If the `parent` is `None`, the `target` node will be a root.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// let root2 = tree.create(None).unwrap();
    /// // move `root2` to be a child of `root`.
    /// tree.mov(root2, root).unwrap();
    /// ```
    pub fn mov<T: Into<Option<TreeID>>>(&self, target: TreeID, parent: T) -> LoroResult<()> {
        self.handler.mov(target, parent.into())
    }

    /// Delete a tree node.
    ///
    /// Note: If the deleted node has children, the children do not appear in the state
    /// rather than actually being deleted.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// tree.delete(root).unwrap();
    /// ```
    pub fn delete(&self, target: TreeID) -> LoroResult<()> {
        self.handler.delete(target)
    }

    /// Get the associated metadata map handler of a tree node.
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// let root_meta = tree.get_meta(root).unwrap();
    /// root_meta.insert("color", "red");
    /// ```
    pub fn get_meta(&self, target: TreeID) -> LoroResult<LoroMap> {
        self.handler
            .get_meta(target)
            .map(|h| LoroMap { handler: h })
    }

    /// Return the parent of target node.
    ///
    /// - If the target node does not exist, return `None`.
    /// - If the target node is a root node, return `Some(None)`.
    pub fn parent(&self, target: TreeID) -> Option<Option<TreeID>> {
        self.handler.parent(target)
    }

    /// Return whether target node exists.
    pub fn contains(&self, target: TreeID) -> bool {
        self.handler.contains(target)
    }

    /// Return all nodes
    pub fn nodes(&self) -> Vec<TreeID> {
        self.handler.nodes()
    }

    /// Return the flat array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use [TreeHandler::get_value_with_meta()].
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    /// Return the flat array of the forest, each node is with metadata.
    pub fn get_value_with_meta(&self) -> LoroValue {
        self.handler.get_deep_value()
    }
}

use enum_as_inner::EnumAsInner;

/// All the CRDT containers supported by loro.
#[derive(Clone, Debug, EnumAsInner)]
pub enum Container {
    List(LoroList),
    Map(LoroMap),
    Text(LoroText),
    Tree(LoroTree),
}

impl From<InnerHandler> for Container {
    fn from(value: InnerHandler) -> Self {
        match value {
            InnerHandler::Text(x) => Container::Text(LoroText { handler: x }),
            InnerHandler::Map(x) => Container::Map(LoroMap { handler: x }),
            InnerHandler::List(x) => Container::List(LoroList { handler: x }),
            InnerHandler::Tree(x) => Container::Tree(LoroTree { handler: x }),
        }
    }
}
