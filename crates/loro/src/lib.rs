#![doc = include_str!("../README.md")]
use either::Either;
use event::{DiffEvent, Subscriber};
use loro_internal::change::Timestamp;
use loro_internal::container::IntoContainerId;
use loro_internal::encoding::ImportBlobMetadata;
use loro_internal::handler::HandlerTrait;
use loro_internal::handler::ValueOrHandler;
use loro_internal::LoroDoc as InnerLoroDoc;
use loro_internal::OpLog;

use loro_internal::{
    handler::Handler as InnerHandler, ListHandler as InnerListHandler,
    MapHandler as InnerMapHandler, TextHandler as InnerTextHandler,
    TreeHandler as InnerTreeHandler,
};
use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

pub mod event;

pub use loro_internal::configure::Configure;
pub use loro_internal::configure::StyleConfigMap;
pub use loro_internal::container::{richtext::ExpandType, FracIndex};
pub use loro_internal::container::{ContainerID, ContainerType};
pub use loro_internal::delta::{TreeDeltaItem, TreeDiff, TreeExternalDiff};
pub use loro_internal::event::Index;
pub use loro_internal::handler::TextDelta;
pub use loro_internal::id::{PeerID, TreeID, ID};
pub use loro_internal::obs::SubID;
pub use loro_internal::oplog::FrontiersNotIncluded;
pub use loro_internal::version::{Frontiers, VersionVector};
pub use loro_internal::{loro_value, to_value};
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

    /// Get the configureations of the document.
    pub fn config(&self) -> &Configure {
        self.doc.config()
    }

    /// Decodes the metadata for an imported blob from the provided bytes.
    pub fn decode_import_blob_meta(bytes: &[u8]) -> LoroResult<ImportBlobMetadata> {
        InnerLoroDoc::decode_import_blob_meta(bytes)
    }

    /// Set whether to record the timestamp of each change. Default is `false`.
    ///
    /// If enabled, the Unix timestamp will be recorded for each change automatically.
    ///
    /// You can set each timestamp manually when commiting a change.
    ///
    /// NOTE: Timestamps are forced to be in ascending order.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    #[inline]
    pub fn set_record_timestamp(&self, record: bool) {
        self.doc.set_record_timestamp(record);
    }

    /// Set the interval of mergeable changes.
    ///
    /// If two continuous local changes are within the interval, they will be merged into one change.
    /// The defualt value is 1000 seconds.
    #[inline]
    pub fn set_change_merge_interval(&self, interval: i64) {
        self.doc.set_change_merge_interval(interval);
    }

    /// Set the rich text format configuration of the document.
    ///
    /// You need to config it if you use rich text `mark` method.
    /// Specifically, you need to config the `expand` property of each style.
    ///
    /// Expand is used to specify the behavior of expanding when new text is inserted at the
    /// beginning or end of the style.
    pub fn config_text_style(&self, text_style: StyleConfigMap) {
        self.doc.config_text_style(text_style)
    }

    /// Attach the document state to the latest known version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    pub fn attach(&self) {
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
    pub fn checkout(&self, frontiers: &Frontiers) -> LoroResult<()> {
        self.doc.checkout(frontiers)
    }

    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        self.doc.cmp_with_frontiers(other)
    }

    /// Compare two frontiers.
    ///
    /// If the frontiers are not included in the document, return `FrontiersNotIncluded`.
    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        self.doc.cmp_frontiers(a, b)
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

    /// Commit the cumulative auto commit transaction with custom configure.
    ///
    /// There is a transaction behind every operation.
    /// It will automatically commit when users invoke export or import.
    /// The event will be sent after a transaction is committed
    pub fn commit_with(
        &self,
        origin: Option<&str>,
        timestamp: Option<Timestamp>,
        immediate_renew: bool,
    ) {
        self.doc
            .commit_with(origin.map(|x| x.into()), timestamp, immediate_renew)
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

    pub fn with_oplog<R>(&self, f: impl FnOnce(&OpLog) -> R) -> R {
        let oplog = self.doc.oplog().lock().unwrap();
        f(&oplog)
    }

    /// Get the `VersionVector` version of `OpLog`
    pub fn oplog_vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Get the `VersionVector` version of `OpLog`
    pub fn state_vv(&self) -> VersionVector {
        self.doc.state_vv()
    }

    /// Get the total number of operations in the `OpLog`
    pub fn len_ops(&self) -> usize {
        self.doc.len_ops()
    }

    /// Get the total number of changes in the `OpLog`
    pub fn len_changes(&self) -> usize {
        self.doc.len_changes()
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

    /// Get the PeerID
    pub fn peer_id(&self) -> PeerID {
        self.doc.peer_id()
    }

    /// Change the PeerID
    ///
    /// NOTE: You need ot make sure there is no chance two peer have the same PeerID.
    /// If it happens, the document will be corrupted.
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        self.doc.set_peer_id(peer)
    }

    /// Subscribe the events of a container.
    ///
    /// The callback will be invoked when the container is changed.
    /// Returns a subscription id that can be used to unsubscribe.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::LoroDoc;
    /// # use std::sync::{atomic::AtomicBool, Arc};
    /// # use loro::{event::DiffEvent, LoroResult, TextDelta};
    /// #
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// let ran = Arc::new(AtomicBool::new(false));
    /// let ran2 = ran.clone();
    /// doc.subscribe(
    ///     &text.id(),
    ///     Arc::new(move |event| {
    ///         assert!(event.local);
    ///         for event in event.events {
    ///             let delta = event.diff.as_text().unwrap();
    ///             let d = TextDelta::Insert {
    ///                 insert: "123".into(),
    ///                 attributes: Default::default(),
    ///             };
    ///             assert_eq!(delta, &vec![d]);
    ///             ran2.store(true, std::sync::atomic::Ordering::Relaxed);
    ///         }
    ///     }),
    /// );
    /// text.insert(0, "123").unwrap();
    /// doc.commit();
    /// assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
    /// ```
    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> SubID {
        self.doc.subscribe(
            container_id,
            Arc::new(move |e| {
                callback(DiffEvent::from(e));
            }),
        )
    }

    /// Subscribe all the events.
    ///
    /// The callback will be invoked when any part of the [loro_internal::DocState] is changed.
    /// Returns a subscription id that can be used to unsubscribe.
    pub fn subscribe_root(&self, callback: Subscriber) -> SubID {
        // self.doc.subscribe_root(callback)
        self.doc.subscribe_root(Arc::new(move |e| {
            callback(DiffEvent::from(e));
        }))
    }

    /// Remove a subscription.
    pub fn unsubscribe(&self, id: SubID) {
        self.doc.unsubscribe(id)
    }

    pub fn log_estimate_size(&self) {
        self.doc.log_estimated_size();
    }

    /// Get the handler by the path.
    pub fn get_by_path(&self, path: &[Index]) -> Option<ValueOrContainer> {
        self.doc.get_by_path(path).map(ValueOrContainer::from)
    }

    /// Get the handler by the string path.
    pub fn get_by_str_path(&self, path: &str) -> Option<ValueOrContainer> {
        self.doc.get_by_str_path(path).map(ValueOrContainer::from)
    }
}

/// It's used to prevent the user from implementing the trait directly.
#[allow(private_bounds)]
trait SealedTrait {}
#[allow(private_bounds)]
pub trait ContainerTrait: SealedTrait {
    type Handler: HandlerTrait;
    fn to_container(&self) -> Container;
    fn to_handler(&self) -> Self::Handler;
    fn from_handler(handler: Self::Handler) -> Self;
    fn try_from_container(container: Container) -> Option<Self>
    where
        Self: Sized;
    fn is_attached(&self) -> bool;
    /// If a detached container is attached, this method will return its corresponding attached handler.
    fn get_attached(&self) -> Option<Self>
    where
        Self: Sized;
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

impl SealedTrait for LoroList {}
impl ContainerTrait for LoroList {
    type Handler = InnerListHandler;
    fn to_container(&self) -> Container {
        Container::List(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self> {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn try_from_container(container: Container) -> Option<Self> {
        container.into_list().ok()
    }
}

impl LoroList {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new() -> Self {
        Self {
            handler: InnerListHandler::new_detached(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

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
            Some(ValueOrHandler::Handler(c)) => Some(Either::Right(c.into())),
            Some(ValueOrHandler::Value(v)) => Some(Either::Left(v)),
            None => None,
        }
    }

    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    #[inline]
    pub fn id(&self) -> ContainerID {
        self.handler.id().clone()
    }

    #[inline]
    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.handler.pop()
    }

    #[inline]
    pub fn push(&self, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.push(v.into())
    }

    #[inline]
    pub fn push_container<C: ContainerTrait>(&self, child: C) -> LoroResult<C> {
        let pos = self.handler.len();
        Ok(C::from_handler(
            self.handler.insert_container(pos, child.to_handler())?,
        ))
    }

    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut(ValueOrHandler),
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
    /// # use loro::{LoroDoc, ContainerType, LoroText, ToJson};
    /// # use serde_json::json;
    /// let doc = LoroDoc::new();
    /// let list = doc.get_list("m");
    /// let text = list.insert_container(0, LoroText::new()).unwrap();
    /// text.insert(0, "12");
    /// text.insert(0, "0");
    /// assert_eq!(doc.get_deep_value().to_json_value(), json!({"m": ["012"]}));
    /// ```
    #[inline]
    pub fn insert_container<C: ContainerTrait>(&self, pos: usize, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler.insert_container(pos, child.to_handler())?,
        ))
    }
}

impl Default for LoroList {
    fn default() -> Self {
        Self::new()
    }
}

/// LoroMap container.
///
/// It's LWW(Last-Write-Win) Map. It can support Multi-Value Map in the future.
///
/// # Example
/// ```
/// # use loro::{LoroDoc, ToJson, ExpandType, LoroText, LoroValue};
/// # use serde_json::json;
/// let doc = LoroDoc::new();
/// let map = doc.get_map("map");
/// map.insert("key", "value").unwrap();
/// map.insert("true", true).unwrap();
/// map.insert("null", LoroValue::Null).unwrap();
/// map.insert("deleted", LoroValue::Null).unwrap();
/// map.delete("deleted").unwrap();
/// let text = map
///    .insert_container("text", LoroText::new()).unwrap();
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

impl SealedTrait for LoroMap {}
impl ContainerTrait for LoroMap {
    type Handler = InnerMapHandler;

    fn to_container(&self) -> Container {
        Container::Map(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self> {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn try_from_container(container: Container) -> Option<Self> {
        container.into_map().ok()
    }
}

impl LoroMap {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new() -> Self {
        Self {
            handler: InnerMapHandler::new_detached(),
        }
    }

    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    pub fn delete(&self, key: &str) -> LoroResult<()> {
        self.handler.delete(key)
    }

    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut(&str, ValueOrHandler),
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
        self.handler.id().clone()
    }

    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<Either<LoroValue, Container>> {
        match self.handler.get_(key) {
            None => None,
            Some(ValueOrHandler::Handler(c)) => Some(Either::Right(c.into())),
            Some(ValueOrHandler::Value(v)) => Some(Either::Left(v)),
        }
    }

    /// Insert a container with the given type at the given key.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, LoroText, ContainerType, ToJson};
    /// # use serde_json::json;
    /// let doc = LoroDoc::new();
    /// let map = doc.get_map("m");
    /// let text = map.insert_container("t", LoroText::new()).unwrap();
    /// text.insert(0, "12");
    /// text.insert(0, "0");
    /// assert_eq!(doc.get_deep_value().to_json_value(), json!({"m": {"t": "012"}}));
    /// ```
    pub fn insert_container<C: ContainerTrait>(&self, key: &str, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler.insert_container(key, child.to_handler())?,
        ))
    }

    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    pub fn get_or_create_container<C: ContainerTrait>(&self, key: &str, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler
                .get_or_create_container(key, child.to_handler())?,
        ))
    }
}

impl Default for LoroMap {
    fn default() -> Self {
        Self::new()
    }
}

/// LoroText container. It's used to model plaintext/richtext.
#[derive(Clone, Debug)]
pub struct LoroText {
    handler: InnerTextHandler,
}

impl SealedTrait for LoroText {}
impl ContainerTrait for LoroText {
    type Handler = InnerTextHandler;

    fn to_container(&self) -> Container {
        Container::Text(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self> {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn try_from_container(container: Container) -> Option<Self> {
        container.into_text().ok()
    }
}

impl LoroText {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new() -> Self {
        Self {
            handler: InnerTextHandler::new_detached(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Get the [ContainerID]  of the text container.
    pub fn id(&self) -> ContainerID {
        self.handler.id().clone()
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
        key: &str,
        value: impl Into<LoroValue>,
    ) -> LoroResult<()> {
        self.handler.mark(range.start, range.end, key, value.into())
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
    pub fn unmark(&self, range: Range<usize>, key: &str) -> LoroResult<()> {
        self.handler.unmark(range.start, range.end, key)
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
    /// text.mark(0..5, "bold", true).unwrap();
    /// assert_eq!(
    ///     text.to_delta().to_json_value(),
    ///     json!([
    ///         { "insert": "Hello", "attributes": {"bold": true} },
    ///         { "insert": " world!" },
    ///     ])
    /// );
    /// text.unmark(3..5, "bold").unwrap();
    /// assert_eq!(
    ///     text.to_delta().to_json_value(),
    ///     json!([
    ///         { "insert": "Hel", "attributes": {"bold": true} },
    ///         { "insert": "lo world!" },
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

impl Default for LoroText {
    fn default() -> Self {
        Self::new()
    }
}

/// LoroTree container. It's used to model movable trees.
///
/// You may use it to model directories, outline or other movable hierarchical data.
#[derive(Clone, Debug)]
pub struct LoroTree {
    handler: InnerTreeHandler,
}

impl SealedTrait for LoroTree {}
impl ContainerTrait for LoroTree {
    type Handler = InnerTreeHandler;

    fn to_container(&self) -> Container {
        Container::Tree(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self> {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn try_from_container(container: Container) -> Option<Self> {
        container.into_tree().ok()
    }
}

impl LoroTree {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new() -> Self {
        Self {
            handler: InnerTreeHandler::new_detached(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

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
        let parent = parent.into();
        let index = self.children_len(parent).unwrap_or(0);
        self.handler.create(parent, index)
    }

    pub fn create_at<T: Into<Option<TreeID>>>(
        &self,
        parent: T,
        index: usize,
    ) -> LoroResult<TreeID> {
        self.handler.create(parent, index)
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
        let parent = parent.into();
        let index = self.children_len(parent).unwrap_or(0);
        self.handler.mov(target, parent, index)
    }

    pub fn mov_to<T: Into<Option<TreeID>>>(
        &self,
        target: TreeID,
        parent: T,
        to: usize,
    ) -> LoroResult<()> {
        let parent = parent.into();
        self.handler.mov(target, parent, to)
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
        self.handler.get_node_parent(target)
    }

    /// Return whether target node exists.
    pub fn contains(&self, target: TreeID) -> bool {
        self.handler.contains(target)
    }

    /// Return all nodes
    pub fn nodes(&self) -> Vec<TreeID> {
        self.handler.nodes()
    }

    pub fn children(&self, parent: Option<TreeID>) -> Vec<TreeID> {
        self.handler.children(parent)
    }

    pub fn children_len(&self, parent: Option<TreeID>) -> Option<usize> {
        self.handler.children_len(parent)
    }

    /// Return container id of the tree.
    pub fn id(&self) -> ContainerID {
        self.handler.id().clone()
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

    #[cfg(feature = "test_utils")]
    pub fn next_tree_id(&self) -> TreeID {
        self.handler.next_tree_id()
    }
}

impl Default for LoroTree {
    fn default() -> Self {
        Self::new()
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

impl SealedTrait for Container {}
impl ContainerTrait for Container {
    type Handler = loro_internal::handler::Handler;

    fn to_container(&self) -> Container {
        self.clone()
    }

    fn to_handler(&self) -> Self::Handler {
        match self {
            Container::List(x) => Self::Handler::List(x.to_handler()),
            Container::Map(x) => Self::Handler::Map(x.to_handler()),
            Container::Text(x) => Self::Handler::Text(x.to_handler()),
            Container::Tree(x) => Self::Handler::Tree(x.to_handler()),
        }
    }

    fn from_handler(handler: Self::Handler) -> Self {
        match handler {
            InnerHandler::Text(x) => Container::Text(LoroText { handler: x }),
            InnerHandler::Map(x) => Container::Map(LoroMap { handler: x }),
            InnerHandler::List(x) => Container::List(LoroList { handler: x }),
            InnerHandler::Tree(x) => Container::Tree(LoroTree { handler: x }),
        }
    }

    fn is_attached(&self) -> bool {
        match self {
            Container::List(x) => x.is_attached(),
            Container::Map(x) => x.is_attached(),
            Container::Text(x) => x.is_attached(),
            Container::Tree(x) => x.is_attached(),
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match self {
            Container::List(x) => x.get_attached().map(Container::List),
            Container::Map(x) => x.get_attached().map(Container::Map),
            Container::Text(x) => x.get_attached().map(Container::Text),
            Container::Tree(x) => x.get_attached().map(Container::Tree),
        }
    }

    fn try_from_container(container: Container) -> Option<Self>
    where
        Self: Sized,
    {
        Some(container)
    }
}

impl Container {
    /// Create a detached container of the given type.
    ///
    /// A detached container is a container that is not attached to a document.
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new(kind: ContainerType) -> Self {
        match kind {
            ContainerType::List => Container::List(LoroList::new()),
            ContainerType::Map => Container::Map(LoroMap::new()),
            ContainerType::Text => Container::Text(LoroText::new()),
            ContainerType::Tree => Container::Tree(LoroTree::new()),
        }
    }

    pub fn get_type(&self) -> ContainerType {
        match self {
            Container::List(_) => ContainerType::List,
            Container::Map(_) => ContainerType::Map,
            Container::Text(_) => ContainerType::Text,
            Container::Tree(_) => ContainerType::Tree,
        }
    }

    pub fn id(&self) -> ContainerID {
        match self {
            Container::List(x) => x.id(),
            Container::Map(x) => x.id(),
            Container::Text(x) => x.id(),
            Container::Tree(x) => x.id(),
        }
    }
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

#[derive(Debug, Clone, EnumAsInner)]
pub enum ValueOrContainer {
    Value(LoroValue),
    Container(Container),
}
