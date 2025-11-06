#![doc = include_str!("../README.md")]
#![allow(clippy::uninlined_format_args)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
use event::DiffBatch;
use event::{DiffEvent, Subscriber};
pub use loro_common::InternalString;
pub use loro_internal::cursor::CannotFindRelativePosition;
use loro_internal::cursor::Cursor;
use loro_internal::cursor::PosQueryResult;
use loro_internal::cursor::Side;
pub use loro_internal::encoding::ImportStatus;
use loro_internal::handler::{HandlerTrait, ValueOrHandler};
pub use loro_internal::loro::ChangeTravelError;
pub use loro_internal::pre_commit::{
    ChangeModifier, FirstCommitFromPeerCallback, FirstCommitFromPeerPayload, PreCommitCallback,
    PreCommitCallbackPayload,
};
pub use loro_internal::sync;
pub use loro_internal::undo::{OnPop, UndoItemMeta, UndoOrRedo};
use loro_internal::version::shrink_frontiers;
pub use loro_internal::version::ImVersionVector;
use loro_internal::DocState;
use loro_internal::LoroDoc as InnerLoroDoc;
use loro_internal::OpLog;
use loro_internal::{
    handler::Handler as InnerHandler, ListHandler as InnerListHandler,
    MapHandler as InnerMapHandler, MovableListHandler as InnerMovableListHandler,
    TextHandler as InnerTextHandler, TreeHandler as InnerTreeHandler,
    UnknownHandler as InnerUnknownHandler,
};
use rustc_hash::FxHashSet;
use std::cmp::Ordering;
use std::ops::ControlFlow;
use std::ops::Deref;
use std::ops::Range;
use std::sync::Arc;
use tracing::info;

pub use loro_internal::diff::diff_impl::UpdateOptions;
pub use loro_internal::diff::diff_impl::UpdateTimeoutError;
pub use loro_internal::subscription::LocalUpdateCallback;
pub use loro_internal::subscription::PeerIdUpdateCallback;
pub use loro_internal::ChangeMeta;
pub use loro_internal::LORO_VERSION;
pub mod event;
pub use loro_internal::awareness;
pub use loro_internal::change::Timestamp;
pub use loro_internal::configure::Configure;
pub use loro_internal::configure::{StyleConfig, StyleConfigMap};
pub use loro_internal::container::richtext::ExpandType;
pub use loro_internal::container::{ContainerID, ContainerType, IntoContainerId};
pub use loro_internal::cursor;
pub use loro_internal::delta::{TreeDeltaItem, TreeDiff, TreeDiffItem, TreeExternalDiff};
pub use loro_internal::encoding::ImportBlobMetadata;
pub use loro_internal::encoding::{EncodedBlobMode, ExportMode};
pub use loro_internal::event::{EventTriggerKind, Index};
pub use loro_internal::handler::TextDelta;
pub use loro_internal::json;
pub use loro_internal::json::{
    FutureOp as JsonFutureOp, FutureOpWrapper as JsonFutureOpWrapper, JsonChange, JsonOp,
    JsonOpContent, JsonSchema, ListOp as JsonListOp, MapOp as JsonMapOp,
    MovableListOp as JsonMovableListOp, TextOp as JsonTextOp, TreeOp as JsonTreeOp,
};
pub use loro_internal::kv_store::{KvStore, MemKvStore};
pub use loro_internal::loro::CommitOptions;
pub use loro_internal::loro::DocAnalysis;
pub use loro_internal::oplog::FrontiersNotIncluded;
pub use loro_internal::undo;
pub use loro_internal::version::{Frontiers, VersionRange, VersionVector, VersionVectorDiff};
pub use loro_internal::ApplyDiff;
pub use loro_internal::Subscription;
pub use loro_internal::UndoManager as InnerUndoManager;
pub use loro_internal::{loro_value, to_value};
pub use loro_internal::{
    Counter, CounterSpan, FractionalIndex, IdLp, IdSpan, Lamport, PeerID, TreeID, TreeParentId, ID,
};
pub use loro_internal::{
    LoroBinaryValue, LoroEncodeError, LoroError, LoroListValue, LoroMapValue, LoroResult,
    LoroStringValue, LoroTreeError, LoroValue, ToJson,
};
pub use loro_kv_store as kv_store;

#[cfg(feature = "jsonpath")]
pub use loro_internal::jsonpath;

#[cfg(feature = "counter")]
mod counter;
#[cfg(feature = "counter")]
pub use counter::LoroCounter;

/// `LoroDoc` is the entry for the whole document.
/// When it's dropped, all the associated [`Container`]s will be invalidated.
///
/// **Important:** Loro is a pure library and does not handle network protocols.
/// It is the responsibility of the user to manage the storage, loading, and synchronization
/// of the bytes exported by Loro in a manner suitable for their specific environment.
#[derive(Debug)]
pub struct LoroDoc {
    doc: InnerLoroDoc,
    // This field is here to prevent some weird issues in debug mode
    #[cfg(debug_assertions)]
    _temp: u8,
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for LoroDoc {
    /// This creates a reference clone, not a deep clone. The cloned doc will share the same
    /// underlying doc as the original one.
    ///
    /// For deep clone, please use the `.fork()` method.
    fn clone(&self) -> Self {
        let doc = self.doc.clone();
        LoroDoc::_new(doc)
    }
}

impl LoroDoc {
    #[inline(always)]
    fn _new(doc: InnerLoroDoc) -> Self {
        Self {
            doc,
            #[cfg(debug_assertions)]
            _temp: 0,
        }
    }

    /// Create a new `LoroDoc` instance.
    #[inline]
    pub fn new() -> Self {
        let doc = InnerLoroDoc::default();
        doc.start_auto_commit();

        LoroDoc::_new(doc)
    }

    /// Duplicate the document with a different PeerID
    ///
    /// The time complexity and space complexity of this operation are both O(n),
    ///
    /// When called in detached mode, it will fork at the current state frontiers.
    /// It will have the same effect as `fork_at(&self.state_frontiers())`.
    #[inline]
    pub fn fork(&self) -> Self {
        let doc = self.doc.fork();
        LoroDoc::_new(doc)
    }

    /// Fork the document at the given frontiers.
    ///
    /// The created doc will only contain the history before the specified frontiers.
    pub fn fork_at(&self, frontiers: &Frontiers) -> LoroDoc {
        let new_doc = self.doc.fork_at(frontiers);
        new_doc.start_auto_commit();
        LoroDoc::_new(new_doc)
    }

    /// Get the configurations of the document.
    #[inline]
    pub fn config(&self) -> &Configure {
        self.doc.config()
    }

    /// Get `Change` at the given id.
    ///
    /// `Change` is a grouped continuous operations that share the same id, timestamp, commit message.
    ///
    /// - The id of the `Change` is the id of its first op.
    /// - The second op's id is `{ peer: change.id.peer, counter: change.id.counter + 1 }`
    ///
    /// The same applies on `Lamport`:
    ///
    /// - The lamport of the `Change` is the lamport of its first op.
    /// - The second op's lamport is `change.lamport + 1`
    ///
    /// The length of the `Change` is how many operations it contains
    pub fn get_change(&self, id: ID) -> Option<ChangeMeta> {
        let change = self.doc.oplog().lock().unwrap().get_change_at(id)?;
        Some(ChangeMeta::from_change(&change))
    }

    /// Decodes the metadata for an imported blob from the provided bytes.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ExportMode};
    ///
    /// let doc = LoroDoc::new();
    /// doc.get_text("t").insert(0, "Hello").unwrap();
    /// let updates = doc.export(ExportMode::all_updates()).unwrap();
    /// let meta = LoroDoc::decode_import_blob_meta(&updates, true).unwrap();
    /// assert!(meta.change_num >= 1);
    /// ```
    #[inline]
    pub fn decode_import_blob_meta(
        bytes: &[u8],
        check_checksum: bool,
    ) -> LoroResult<ImportBlobMetadata> {
        InnerLoroDoc::decode_import_blob_meta(bytes, check_checksum)
    }

    /// Set whether to record the timestamp of each change. Default is `false`.
    ///
    /// If enabled, the Unix timestamp will be recorded for each change automatically.
    /// You can also set a timestamp explicitly via [`set_next_commit_timestamp`].
    ///
    /// Important: this is a runtime configuration. It is not serialized into updates or
    /// snapshots. You must reapply it for each new `LoroDoc` you create or load.
    ///
    /// NOTE: Timestamps are forced to be in ascending order. If you commit a new change with
    /// a timestamp earlier than the latest, the largest existing timestamp will be used instead.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// doc.set_record_timestamp(true);
    /// doc.get_text("t").insert(0, "hi").unwrap();
    /// doc.commit();
    /// ```
    #[inline]
    pub fn set_record_timestamp(&self, record: bool) {
        self.doc.set_record_timestamp(record);
    }

    /// Enables editing in detached mode, which is disabled by default.
    ///
    /// The doc enter detached mode after calling `detach` or checking out a non-latest version.
    ///
    /// # Important Notes:
    ///
    /// - This mode uses a different PeerID for each checkout.
    /// - Ensure no concurrent operations share the same PeerID if set manually.
    /// - Importing does not affect the document's state or version; changes are
    ///   recorded in the [OpLog] only. Call `checkout` to apply changes.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let v0 = doc.state_frontiers();
    /// // Make some edits…
    /// doc.get_text("t").insert(0, "Hello").unwrap();
    /// doc.commit();
    ///
    /// // Travel back and enable detached editing
    /// doc.checkout(&v0).unwrap();
    /// assert!(doc.is_detached());
    /// doc.set_detached_editing(true);
    /// doc.get_text("t").insert(0, "old").unwrap();
    /// // Later, re-attach to see latest again
    /// doc.attach();
    /// ```
    #[inline]
    pub fn set_detached_editing(&self, enable: bool) {
        self.doc.set_detached_editing(enable);
    }

    /// Whether editing the doc in detached mode is allowed, which is disabled by
    /// default.
    ///
    /// The doc enter detached mode after calling `detach` or checking out a non-latest version.
    ///
    /// # Important Notes:
    ///
    /// - This mode uses a different PeerID for each checkout.
    /// - Ensure no concurrent operations share the same PeerID if set manually.
    /// - Importing does not affect the document's state or version; changes are
    ///   recorded in the [OpLog] only. Call `checkout` to apply changes.
    #[inline]
    pub fn is_detached_editing_enabled(&self) -> bool {
        self.doc.is_detached_editing_enabled()
    }

    /// Set the interval of mergeable changes, **in seconds**.
    ///
    /// If two continuous local changes are within the interval, they will be merged into one change.
    /// The default value is 1000 seconds.
    ///
    /// By default, we record timestamps in seconds for each change. So if the merge interval is 1, and changes A and B
    /// have timestamps of 3 and 4 respectively, then they will be merged into one change.
    #[inline]
    pub fn set_change_merge_interval(&self, interval: i64) {
        self.doc.set_change_merge_interval(interval);
    }

    /// Set the rich text format configuration of the document.
    ///
    /// Configure the `expand` behavior for marks used by [`LoroText::mark`]/[`LoroText::unmark`].
    /// This controls how marks grow when text is inserted at their boundaries.
    ///
    /// - `after` (default): inserts just after the range expand the mark
    /// - `before`: inserts just before the range expand the mark
    /// - `both`: inserts on either side expand the mark
    /// - `none`: do not expand at boundaries
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, StyleConfigMap, StyleConfig, ExpandType};
    /// let doc = LoroDoc::new();
    /// let mut styles = StyleConfigMap::new();
    /// styles.insert("bold".into(), StyleConfig { expand: ExpandType::After });
    /// doc.config_text_style(styles);
    /// ```
    #[inline]
    pub fn config_text_style(&self, text_style: StyleConfigMap) {
        self.doc.config_text_style(text_style)
    }

    /// Configures the default text style for the document.
    ///
    /// This method sets the default text style configuration for the document when using LoroText.
    /// If `None` is provided, the default style is reset.
    ///
    /// # Parameters
    ///
    /// - `text_style`: The style configuration to set as the default. `None` to reset.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, StyleConfig, ExpandType};
    /// let doc = LoroDoc::new();
    /// doc.config_default_text_style(Some(StyleConfig { expand: ExpandType::After }));
    /// ```
    pub fn config_default_text_style(&self, text_style: Option<StyleConfig>) {
        self.doc.config_default_text_style(text_style);
    }

    /// Attach the document state to the latest known version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    #[inline]
    pub fn attach(&self) {
        self.doc.attach()
    }

    /// Checkout the `DocState` to a specific version.
    ///
    /// The document becomes detached during a `checkout` operation.
    /// Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// In a detached state, the document is not editable, and any `import` operations will be
    /// recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// You should call `attach` (or `checkout_to_latest`) to reattach the `DocState` to the latest version of `OpLog`.
    /// If you need to edit while detached, enable [`set_detached_editing(true)`], but note it uses a different
    /// PeerID per checkout.
    #[inline]
    pub fn checkout(&self, frontiers: &Frontiers) -> LoroResult<()> {
        self.doc.checkout(frontiers)
    }

    /// Checkout the `DocState` to the latest version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// This has the same effect as `attach`.
    #[inline]
    pub fn checkout_to_latest(&self) {
        self.doc.checkout_to_latest()
    }

    /// Compare the frontiers with the current OpLog's version.
    ///
    /// If `other` contains any version that's not contained in the current OpLog, return [Ordering::Less].
    #[inline]
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        self.doc.cmp_with_frontiers(other)
    }

    /// Compare two frontiers.
    ///
    /// If the frontiers are not included in the document, return [`FrontiersNotIncluded`].
    #[inline]
    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        self.doc.cmp_frontiers(a, b)
    }

    /// Force the document enter the detached mode.
    ///
    /// In this mode, importing new updates only records them in the OpLog; the [loro_internal::DocState] is not updated until you reattach.
    ///
    /// Learn more at https://loro.dev/docs/advanced/doc_state_and_oplog#attacheddetached-status
    #[inline]
    pub fn detach(&self) {
        self.doc.detach()
    }

    /// Import a batch of updates/snapshot.
    ///
    /// The data can be in arbitrary order. The import result will be the same.
    /// Auto-commit: same as [`import`], this finalizes the current transaction first.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ExportMode};
    /// let a = LoroDoc::new();
    /// a.get_text("t").insert(0, "A").unwrap();
    /// let u1 = a.export(ExportMode::all_updates()).unwrap();
    /// a.get_text("t").insert(1, "B").unwrap();
    /// let u2 = a.export(ExportMode::all_updates()).unwrap();
    ///
    /// let b = LoroDoc::new();
    /// let status = b.import_batch(&[u2, u1]).unwrap(); // arbitrary order
    /// assert!(status.pending.is_none());
    /// ```
    #[inline]
    pub fn import_batch(&self, bytes: &[Vec<u8>]) -> LoroResult<ImportStatus> {
        self.doc.import_batch(bytes)
    }

    /// Get a [Container] by container id.
    #[inline]
    pub fn get_container(&self, id: ContainerID) -> Option<Container> {
        self.doc.get_handler(id).map(Container::from_handler)
    }

    /// Get a [LoroMovableList] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    #[inline]
    pub fn get_movable_list<I: IntoContainerId>(&self, id: I) -> LoroMovableList {
        LoroMovableList {
            handler: self.doc.get_movable_list(id),
        }
    }

    /// Get a [LoroList] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    /// Note: creating/accessing a root container does not record history; creating nested
    /// containers (e.g., `Map::insert_container`) does.
    #[inline]
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> LoroList {
        LoroList {
            handler: self.doc.get_list(id),
        }
    }

    /// Get a [LoroMap] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    /// Note: creating/accessing a root container does not record history; creating nested
    /// containers (e.g., `Map::insert_container`) does.
    #[inline]
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> LoroMap {
        LoroMap {
            handler: self.doc.get_map(id),
        }
    }

    /// Get a [LoroText] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    /// Note: creating/accessing a root container does not record history; creating nested
    /// containers (e.g., `Map::insert_container`) does.
    #[inline]
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> LoroText {
        LoroText {
            handler: self.doc.get_text(id),
        }
    }

    /// Get a [LoroTree] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    /// Note: creating/accessing a root container does not record history; creating nested
    /// containers (e.g., `Map::insert_container`) does.
    #[inline]
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> LoroTree {
        LoroTree {
            handler: self.doc.get_tree(id),
        }
    }

    #[cfg(feature = "counter")]
    /// Get a [LoroCounter] by container id.
    ///
    /// If the provided id is string, it will be converted into a root container id with the name of the string.
    #[inline]
    pub fn get_counter<I: IntoContainerId>(&self, id: I) -> LoroCounter {
        LoroCounter {
            handler: self.doc.get_counter(id),
        }
    }

    /// Commit the cumulative auto commit transaction.
    ///
    /// There is a transaction behind every operation.
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// Note: Loro transactions are not ACID database transactions. There is no rollback or
    /// isolation; they are a grouping mechanism for events/history. For interactive undo/redo,
    /// use [`UndoManager`].
    ///
    /// Empty-commit behavior: this method is an explicit commit. If the pending
    /// transaction is empty, any previously set next-commit options (message/timestamp/origin)
    /// are swallowed and will not carry over.
    #[inline]
    pub fn commit(&self) {
        self.doc.commit_then_renew();
    }

    /// Commit the cumulative auto commit transaction with custom options.
    ///
    /// There is a transaction behind every operation.
    /// It will automatically commit when users invoke export or import.
    /// The event will be sent after a transaction is committed
    ///
    /// See also: [`set_next_commit_message`], [`set_next_commit_origin`],
    /// [`set_next_commit_timestamp`]. Commit messages are persisted and replicate to peers;
    /// origins are local-only metadata.
    ///
    /// Empty-commit behavior: this method is an explicit commit. If the pending
    /// transaction is empty, the provided options are swallowed and will not carry over.
    /// For implicit commits triggered by `export`/`checkout` (commit barriers),
    /// message/timestamp/origin from an empty transaction are preserved for the next commit.
    #[inline]
    pub fn commit_with(&self, options: CommitOptions) {
        self.doc.commit_with(options);
    }

    /// Set commit message for the current uncommitted changes
    ///
    /// It will be persisted.
    pub fn set_next_commit_message(&self, msg: &str) {
        self.doc.set_next_commit_message(msg)
    }

    /// Set `origin` for the current uncommitted changes, it can be used to track the source of changes in an event.
    ///
    /// It will NOT be persisted.
    pub fn set_next_commit_origin(&self, origin: &str) {
        self.doc.set_next_commit_origin(origin)
    }

    /// Set the timestamp of the next commit.
    ///
    /// It will be persisted and stored in the `OpLog`.
    /// You can get the timestamp from the [`Change`] type.
    pub fn set_next_commit_timestamp(&self, timestamp: Timestamp) {
        self.doc.set_next_commit_timestamp(timestamp)
    }

    /// Set the options of the next commit.
    ///
    /// It will be used when the next commit is performed.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, CommitOptions};
    /// let doc = LoroDoc::new();
    /// doc.set_next_commit_options(CommitOptions::new().origin("ui").commit_msg("tagged"));
    /// doc.get_text("t").insert(0, "x").unwrap();
    /// doc.commit();
    /// ```
    pub fn set_next_commit_options(&self, options: CommitOptions) {
        self.doc.set_next_commit_options(options);
    }

    /// Clear the options of the next commit.
    pub fn clear_next_commit_options(&self) {
        self.doc.clear_next_commit_options();
    }

    /// Whether the document is in detached mode, where the [loro_internal::DocState] is not
    /// synchronized with the latest version of the [loro_internal::OpLog].
    #[inline]
    pub fn is_detached(&self) -> bool {
        self.doc.is_detached()
    }

    /// Create a new `LoroDoc` from a snapshot.
    ///
    /// The snapshot is created via [`LoroDoc::export`] with [`ExportMode::Snapshot`].
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ExportMode};
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello").unwrap();
    /// let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    ///
    /// let restored = LoroDoc::from_snapshot(&snapshot).unwrap();
    /// assert_eq!(restored.get_deep_value(), doc.get_deep_value());
    /// ```
    pub fn from_snapshot(bytes: &[u8]) -> LoroResult<Self> {
        let inner = InnerLoroDoc::from_snapshot(bytes)?;
        inner.start_auto_commit();
        Ok(Self::_new(inner))
    }

    /// Import data exported by [`LoroDoc::export`].
    ///
    /// Use [`ExportMode::Snapshot`] for full-state snapshots, or
    /// [`ExportMode::all_updates`] / [`ExportMode::updates`] for updates.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ExportMode};
    ///
    /// let a = LoroDoc::new();
    /// a.get_text("text").insert(0, "Hello").unwrap();
    /// let updates = a.export(ExportMode::all_updates()).unwrap();
    ///
    /// let b = LoroDoc::new();
    /// b.import(&updates).unwrap();
    /// assert_eq!(a.get_deep_value(), b.get_deep_value());
    /// ```
    /// Pitfalls:
    /// - Missing dependencies: check the returned [`ImportStatus`]. If `pending` is non-empty,
    ///   fetch those missing ranges (e.g., using `export(ExportMode::updates(&doc.oplog_vv()))`) and re-import.
    /// - Auto-commit: `import` finalizes the current transaction before applying incoming data.
    #[inline]
    pub fn import(&self, bytes: &[u8]) -> Result<ImportStatus, LoroError> {
        self.doc.import_with(bytes, "".into())
    }

    /// Import data exported by [`LoroDoc::export`] and mark it with a custom origin.
    ///
    /// The `origin` string will be attached to the ensuing change event, which is handy
    /// for telemetry or filtering.
    /// Pitfalls:
    /// - Same as [`import`]: verify `ImportStatus.pending` and fetch dependencies if needed.
    #[inline]
    pub fn import_with(&self, bytes: &[u8], origin: &str) -> Result<ImportStatus, LoroError> {
        self.doc.import_with(bytes, origin.into())
    }

    /// Import the json schema updates.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, VersionVector};
    /// let a = LoroDoc::new();
    /// a.get_text("t").insert(0, "hi").unwrap();
    /// a.commit();
    /// let json = a.export_json_updates(&VersionVector::default(), &a.oplog_vv());
    ///
    /// let b = LoroDoc::new();
    /// b.import_json_updates(json).unwrap();
    /// assert_eq!(a.get_deep_value(), b.get_deep_value());
    /// ```
    #[inline]
    pub fn import_json_updates<T: TryInto<JsonSchema>>(
        &self,
        json: T,
    ) -> Result<ImportStatus, LoroError> {
        self.doc.import_json_updates(json)
    }

    /// Export the current state with json-string format of the document.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, VersionVector};
    /// let doc = LoroDoc::new();
    /// let start = VersionVector::default();
    /// let end = doc.oplog_vv();
    /// let json = doc.export_json_updates(&start, &end);
    /// ```
    #[inline]
    pub fn export_json_updates(
        &self,
        start_vv: &VersionVector,
        end_vv: &VersionVector,
    ) -> JsonSchema {
        self.doc.export_json_updates(start_vv, end_vv, true)
    }

    /// Export the current state with json-string format of the document, without peer compression.
    ///
    /// Compared to [`export_json_updates`], this method does not compress the peer IDs in the updates.
    /// So the operations are easier to be processed by application code.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, VersionVector};
    /// let doc = LoroDoc::new();
    /// let start = VersionVector::default();
    /// let end = doc.oplog_vv();
    /// let json = doc.export_json_updates_without_peer_compression(&start, &end);
    /// ```
    #[inline]
    pub fn export_json_updates_without_peer_compression(
        &self,
        start_vv: &VersionVector,
        end_vv: &VersionVector,
    ) -> JsonSchema {
        self.doc.export_json_updates(start_vv, end_vv, false)
    }

    /// Exports changes within the specified ID span to JSON schema format.
    ///
    /// The JSON schema format produced by this method is identical to the one generated by `export_json_updates`.
    /// It ensures deterministic output, making it ideal for hash calculations and integrity checks.
    ///
    /// This method can also export pending changes from the uncommitted transaction that have not yet been applied to the OpLog.
    ///
    /// This method will NOT trigger a new commit implicitly.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, IdSpan};
    ///
    /// let doc = LoroDoc::new();
    /// doc.set_peer_id(0).unwrap();
    /// doc.get_text("text").insert(0, "a").unwrap();
    /// doc.commit();
    /// let doc_clone = doc.clone();
    /// let _sub = doc.subscribe_pre_commit(Box::new(move |e| {
    ///     let changes = doc_clone.export_json_in_id_span(IdSpan::new(
    ///         0,
    ///         0,
    ///         e.change_meta.id.counter + e.change_meta.len as i32,
    ///     ));
    ///     // 2 because commit one and the uncommit one
    ///     assert_eq!(changes.len(), 2);
    ///     true
    /// }));
    /// doc.get_text("text").insert(0, "b").unwrap();
    /// let changes = doc.export_json_in_id_span(IdSpan::new(0, 0, 2));
    /// assert_eq!(changes.len(), 1);
    /// doc.commit();
    /// // change merged
    /// assert_eq!(changes.len(), 1);
    /// ```
    pub fn export_json_in_id_span(&self, id_span: IdSpan) -> Vec<JsonChange> {
        self.doc.export_json_in_id_span(id_span)
    }

    /// Convert `Frontiers` into `VersionVector`
    ///
    /// Returns `None` if the frontiers are not included by this doc's OpLog.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let f = doc.state_frontiers();
    /// let vv = doc.frontiers_to_vv(&f);
    /// assert!(vv.is_some());
    /// ```
    #[inline]
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        self.doc.frontiers_to_vv(frontiers)
    }

    /// Minimize the frontiers by removing the unnecessary entries.
    ///
    /// Returns `Err(ID)` if any frontier is not included by this doc's history.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let f = doc.state_frontiers();
    /// let _minimized = doc.minimize_frontiers(&f).unwrap();
    /// ```
    pub fn minimize_frontiers(&self, frontiers: &Frontiers) -> Result<Frontiers, ID> {
        self.with_oplog(|oplog| shrink_frontiers(frontiers, oplog.dag()))
    }

    /// Convert `VersionVector` into `Frontiers`
    #[inline]
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        self.doc.vv_to_frontiers(vv)
    }

    /// Access the `OpLog`.
    ///
    /// NOTE: The API in `OpLog` is unstable. Keep the closure short; avoid calling methods
    /// that might re-enter the document while holding the lock.
    #[inline]
    pub fn with_oplog<R>(&self, f: impl FnOnce(&OpLog) -> R) -> R {
        let oplog = self.doc.oplog().lock().unwrap();
        f(&oplog)
    }

    /// Access the `DocState`.
    ///
    /// NOTE: The API in `DocState` is unstable. Keep the closure short; avoid calling methods
    /// that might re-enter the document while holding the lock.
    #[inline]
    pub fn with_state<R>(&self, f: impl FnOnce(&mut DocState) -> R) -> R {
        let mut state = self.doc.app_state().lock().unwrap();
        f(&mut state)
    }

    /// Get the `VersionVector` version of `OpLog`
    #[inline]
    pub fn oplog_vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Get the `VersionVector` version of `DocState`
    #[inline]
    pub fn state_vv(&self) -> VersionVector {
        self.doc.state_vv()
    }

    /// The doc only contains the history since this version
    ///
    /// This is empty if the doc is not shallow.
    ///
    /// The ops included by the shallow history start version vector are not in the doc.
    #[inline]
    pub fn shallow_since_vv(&self) -> ImVersionVector {
        self.doc.shallow_since_vv()
    }

    /// The doc only contains the history since this version
    ///
    /// This is empty if the doc is not shallow.
    ///
    /// The ops included by the shallow history start frontiers are not in the doc.
    #[inline]
    pub fn shallow_since_frontiers(&self) -> Frontiers {
        self.doc.shallow_since_frontiers()
    }

    /// Get the total number of operations in the `OpLog`
    #[inline]
    pub fn len_ops(&self) -> usize {
        self.doc.len_ops()
    }

    /// Get the total number of changes in the `OpLog`
    #[inline]
    pub fn len_changes(&self) -> usize {
        self.doc.len_changes()
    }

    /// Get the shallow value of the document.
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.doc.get_value()
    }

    /// Get the entire state of the current DocState
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.doc.get_deep_value()
    }

    /// Get the entire state of the current DocState with container id
    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.doc
            .app_state()
            .lock()
            .unwrap()
            .get_deep_value_with_id()
    }

    /// Get the `Frontiers` version of `OpLog`.
    #[inline]
    pub fn oplog_frontiers(&self) -> Frontiers {
        self.doc.oplog_frontiers()
    }

    /// Get the `Frontiers` version of `DocState`.
    ///
    /// When detached or during checkout, `state_frontiers()` may differ from `oplog_frontiers()`.
    /// Learn more about [`Frontiers`](https://loro.dev/docs/advanced/version_deep_dive).
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let before = doc.state_frontiers();
    /// doc.get_text("t").insert(0, "x").unwrap();
    /// let after = doc.state_frontiers();
    /// assert_ne!(before, after);
    /// ```
    #[inline]
    pub fn state_frontiers(&self) -> Frontiers {
        self.doc.state_frontiers()
    }

    /// Get the PeerID
    #[inline]
    pub fn peer_id(&self) -> PeerID {
        self.doc.peer_id()
    }

    /// Change the PeerID
    ///
    /// Pitfalls:
    /// - Never reuse the same PeerID across concurrent writers (multiple tabs/devices). Duplicate
    ///   PeerIDs can produce conflicting OpIDs and corrupt the document.
    /// - Do not assign a fixed PeerID to a user or device without strict single-ownership locking.
    ///   Prefer the default random PeerID per process/session.
    #[inline]
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        self.doc.set_peer_id(peer)
    }

    /// Subscribe the events of a container.
    ///
    /// The callback will be invoked after a transaction that change the container.
    /// Returns a subscription that can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::LoroDoc;
    /// # use loro::ContainerTrait;
    /// # use std::sync::{atomic::AtomicBool, Arc};
    /// # use loro::{event::DiffEvent, LoroResult, TextDelta};
    /// #
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// let ran = Arc::new(AtomicBool::new(false));
    /// let ran2 = ran.clone();
    /// let sub = doc.subscribe(
    ///     &text.id(),
    ///     Arc::new(move |event| {
    ///         assert!(event.triggered_by.is_local());
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
    /// // unsubscribe
    /// sub.unsubscribe();
    /// ```
    #[inline]
    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> Subscription {
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
    /// Returns a subscription that can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    #[inline]
    pub fn subscribe_root(&self, callback: Subscriber) -> Subscription {
        // self.doc.subscribe_root(callback)
        self.doc.subscribe_root(Arc::new(move |e| {
            callback(DiffEvent::from(e));
        }))
    }

    /// Subscribe to local document updates.
    ///
    /// The callback receives encoded update bytes whenever local changes are committed.
    /// This is useful for syncing changes to other document instances or persisting updates.
    ///
    /// **Auto-unsubscription**: If the callback returns `false`, the subscription will be
    /// automatically removed, providing a convenient way to implement one-time or conditional
    /// subscriptions in Rust.
    ///
    /// # Parameters
    /// - `callback`: Function that receives `&Vec<u8>` (encoded updates) and returns `bool`
    ///   - Return `true` to keep the subscription active
    ///   - Return `false` to automatically unsubscribe
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    /// use std::sync::{Arc, Mutex};
    ///
    /// let doc = LoroDoc::new();
    /// let updates = Arc::new(Mutex::new(Vec::new()));
    /// let updates_clone = updates.clone();
    /// let count = Arc::new(Mutex::new(0));
    /// let count_clone = count.clone();
    ///
    /// // Subscribe and collect first 3 updates, then auto-unsubscribe
    /// let sub = doc.subscribe_local_update(Box::new(move |bytes| {
    ///     updates_clone.lock().unwrap().push(bytes.clone());
    ///     let mut c = count_clone.lock().unwrap();
    ///     *c += 1;
    ///     *c < 3  // Auto-unsubscribe after 3 updates
    /// }));
    ///
    /// doc.get_text("text").insert(0, "hello").unwrap();
    /// doc.commit();
    /// ```
    pub fn subscribe_local_update(&self, callback: LocalUpdateCallback) -> Subscription {
        self.doc.subscribe_local_update(callback)
    }

    /// Subscribe to peer ID changes in the document.
    ///
    /// The callback is triggered whenever the document's peer ID is modified.
    /// This is useful for tracking identity changes and updating related state accordingly.
    ///
    /// **Auto-unsubscription**: If the callback returns `false`, the subscription will be
    /// automatically removed, providing a convenient way to implement one-time or conditional
    /// subscriptions in Rust.
    ///
    /// # Parameters
    /// - `callback`: Function that receives `&ID` (the new peer ID) and returns `bool`
    ///   - Return `true` to keep the subscription active
    ///   - Return `false` to automatically unsubscribe
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    /// use std::sync::{Arc, Mutex};
    ///
    /// let doc = LoroDoc::new();
    /// let peer_changes = Arc::new(Mutex::new(Vec::new()));
    /// let changes_clone = peer_changes.clone();
    ///
    /// let sub = doc.subscribe_peer_id_change(Box::new(move |new_peer_id| {
    ///     changes_clone.lock().unwrap().push(*new_peer_id);
    ///     true  // Keep subscription active
    /// }));
    ///
    /// doc.set_peer_id(42).unwrap();
    /// doc.set_peer_id(100).unwrap();
    /// ```
    pub fn subscribe_peer_id_change(&self, callback: PeerIdUpdateCallback) -> Subscription {
        self.doc.subscribe_peer_id_change(callback)
    }

    /// Check the correctness of the document state by comparing it with the state
    /// calculated by applying all the history.
    #[inline]
    pub fn check_state_correctness_slow(&self) {
        self.doc.check_state_diff_calc_consistency_slow()
    }

    /// Get the handler by the path.
    #[inline]
    pub fn get_by_path(&self, path: &[Index]) -> Option<ValueOrContainer> {
        self.doc.get_by_path(path).map(ValueOrContainer::from)
    }

    /// Get the handler by the string path.
    ///
    /// The path can be specified in different ways depending on the container type:
    ///
    /// For Tree:
    /// 1. Using node IDs: `tree/{node_id}/property`
    /// 2. Using indices: `tree/0/1/property`
    ///
    /// For List and MovableList:
    /// - Using indices: `list/0` or `list/1/property`
    ///
    /// For Map:
    /// - Using keys: `map/key` or `map/nested/property`
    ///
    /// For tree structures, index-based paths follow depth-first traversal order.
    /// The indices start from 0 and represent the position of a node among its siblings.
    ///
    /// # Examples
    /// ```
    /// # use loro::{LoroDoc, LoroValue};
    /// let doc = LoroDoc::new();
    ///
    /// // Tree example
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// tree.get_meta(root).unwrap().insert("name", "root").unwrap();
    /// // Access tree by ID or index
    /// let name1 = doc.get_by_str_path(&format!("tree/{}/name", root)).unwrap().into_value().unwrap();
    /// let name2 = doc.get_by_str_path("tree/0/name").unwrap().into_value().unwrap();
    /// assert_eq!(name1, name2);
    ///
    /// // List example
    /// let list = doc.get_list("list");
    /// list.insert(0, "first").unwrap();
    /// list.insert(1, "second").unwrap();
    /// // Access list by index
    /// let item = doc.get_by_str_path("list/0");
    /// assert_eq!(item.unwrap().into_value().unwrap().into_string().unwrap(), "first".into());
    ///
    /// // Map example
    /// let map = doc.get_map("map");
    /// map.insert("key", "value").unwrap();
    /// // Access map by key
    /// let value = doc.get_by_str_path("map/key");
    /// assert_eq!(value.unwrap().into_value().unwrap().into_string().unwrap(), "value".into());
    ///
    /// // MovableList example
    /// let mlist = doc.get_movable_list("mlist");
    /// mlist.insert(0, "item").unwrap();
    /// // Access movable list by index
    /// let item = doc.get_by_str_path("mlist/0");
    /// assert_eq!(item.unwrap().into_value().unwrap().into_string().unwrap(), "item".into());
    /// ```
    #[inline]
    pub fn get_by_str_path(&self, path: &str) -> Option<ValueOrContainer> {
        self.doc.get_by_str_path(path).map(ValueOrContainer::from)
    }

    /// Get the absolute position of the given cursor.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, ToJson};
    /// let doc = LoroDoc::new();
    /// let text = &doc.get_text("text");
    /// text.insert(0, "01234").unwrap();
    /// let pos = text.get_cursor(5, Default::default()).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    /// text.insert(0, "01234").unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 10);
    /// text.delete(0, 10).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 0);
    /// text.insert(0, "01234").unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    /// ```
    #[inline]
    pub fn get_cursor_pos(
        &self,
        cursor: &Cursor,
    ) -> Result<PosQueryResult, CannotFindRelativePosition> {
        self.doc.query_pos(cursor)
    }

    /// Get the inner LoroDoc ref.
    #[inline]
    pub fn inner(&self) -> &InnerLoroDoc {
        &self.doc
    }

    /// Whether the history cache is built.
    #[inline]
    pub fn has_history_cache(&self) -> bool {
        self.doc.has_history_cache()
    }

    /// Free the history cache that is used for making checkout faster.
    ///
    /// If you use checkout that switching to an old/concurrent version, the history cache will be built.
    /// You can free it by calling this method.
    #[inline]
    pub fn free_history_cache(&self) {
        self.doc.free_history_cache()
    }

    /// Free the cached diff calculator that is used for checkout.
    #[inline]
    pub fn free_diff_calculator(&self) {
        self.doc.free_diff_calculator()
    }

    /// Encoded all ops and history cache to bytes and store them in the kv store.
    ///
    /// This will free up the memory that used by parsed ops
    #[inline]
    pub fn compact_change_store(&self) {
        self.doc.compact_change_store()
    }

    /// Export the document in the given mode.
    ///
    /// Common modes:
    /// - [`ExportMode::Snapshot`]: full state + history
    /// - [`ExportMode::all_updates()`]: all known ops
    /// - [`ExportMode::updates(&VersionVector)`]: ops since a specific version
    /// - [`ExportMode::shallow_snapshot(..)`]: GC’d snapshot starting at frontiers
    /// - [`ExportMode::updates_in_range(..)`]: ops in specific ID spans
    ///
    /// Important notes:
    /// - Auto-commit: `export` finalizes the current transaction before producing bytes.
    /// - Shallow snapshots: peers cannot import updates from before the shallow start.
    /// - Performance: exporting fresh snapshots periodically can reduce import time for new peers.
    ///
    /// # Examples
    /// ```
    /// use loro::{ExportMode, LoroDoc};
    ///
    /// let doc = LoroDoc::new();
    /// doc.get_text("text").insert(0, "Hello").unwrap();
    ///
    /// // 1) Full snapshot
    /// let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    ///
    /// // 2) All updates
    /// let all = doc.export(ExportMode::all_updates()).unwrap();
    ///
    /// // 3) Updates from another peer’s version vector
    /// let vv = doc.oplog_vv();
    /// let delta = doc.export(ExportMode::updates(&vv)).unwrap();
    /// assert!(!delta.is_empty());
    /// ```
    pub fn export(&self, mode: ExportMode) -> Result<Vec<u8>, LoroEncodeError> {
        self.doc.export(mode)
    }

    /// Analyze the container info of the doc
    ///
    /// This is used for development and debugging. It can be slow.
    pub fn analyze(&self) -> DocAnalysis {
        self.doc.analyze()
    }

    /// Get the path from the root to the container
    pub fn get_path_to_container(&self, id: &ContainerID) -> Option<Vec<(ContainerID, Index)>> {
        self.doc.get_path_to_container(id)
    }

    /// Evaluate a JSONPath expression on the document and return matching values or handlers.
    ///
    /// This method allows querying the document structure using JSONPath syntax.
    /// It returns a vector of `ValueOrHandler` which can represent either primitive values
    /// or container handlers, depending on what the JSONPath expression matches.
    ///
    /// # Arguments
    ///
    /// * `path` - A string slice containing the JSONPath expression to evaluate.
    ///
    /// # Returns
    ///
    /// A `Result` containing either:
    /// - `Ok(Vec<ValueOrHandler>)`: A vector of matching values or handlers.
    /// - `Err(String)`: An error message if the JSONPath expression is invalid or evaluation fails.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, ToJson};
    ///
    /// let doc = LoroDoc::new();
    /// let map = doc.get_map("users");
    /// map.insert("alice", 30).unwrap();
    /// map.insert("bob", 25).unwrap();
    ///
    /// let result = doc.jsonpath("$.users.alice").unwrap();
    /// assert_eq!(result.len(), 1);
    /// assert_eq!(result[0].as_value().unwrap().to_json_value(), serde_json::json!(30));
    /// ```
    #[inline]
    #[cfg(feature = "jsonpath")]
    pub fn jsonpath(&self, path: &str) -> Result<Vec<ValueOrContainer>, JsonPathError> {
        self.doc
            .jsonpath(path)
            .map(|vec| vec.into_iter().map(ValueOrContainer::from).collect())
    }

    /// Get the number of operations in the pending transaction.
    ///
    /// The pending transaction is the one that is not committed yet. It will be committed
    /// after calling `doc.commit()`, `doc.export(mode)` or `doc.checkout(version)`.
    pub fn get_pending_txn_len(&self) -> usize {
        self.doc.get_pending_txn_len()
    }

    /// Traverses the ancestors of the Change containing the given ID, including itself.
    ///
    /// This method visits all ancestors in causal order, from the latest to the oldest,
    /// based on their Lamport timestamps.
    ///
    /// # Arguments
    ///
    /// * `ids` - The IDs of the Change to start the traversal from.
    /// * `f` - A mutable function that is called for each ancestor. It can return `ControlFlow::Break(())` to stop the traversal.
    pub fn travel_change_ancestors(
        &self,
        ids: &[ID],
        f: &mut dyn FnMut(ChangeMeta) -> ControlFlow<()>,
    ) -> Result<(), ChangeTravelError> {
        self.doc.travel_change_ancestors(ids, f)
    }

    /// Check if the doc contains the full history.
    pub fn is_shallow(&self) -> bool {
        self.doc.is_shallow()
    }

    /// Gets container IDs modified in the given ID range.
    ///
    /// Pitfalls:
    /// - This method will implicitly commit the current transaction to ensure the change range is finalized.
    ///
    /// This method can be used in conjunction with `doc.travel_change_ancestors()` to traverse
    /// the history and identify all changes that affected specific containers.
    ///
    /// # Arguments
    ///
    /// * `id` - The starting ID of the change range
    /// * `len` - The length of the change range to check
    pub fn get_changed_containers_in(&self, id: ID, len: usize) -> FxHashSet<ContainerID> {
        self.doc.get_changed_containers_in(id, len)
    }

    /// Find the operation id spans that between the `from` version and the `to` version.
    ///
    /// Useful for exporting just the changes in a range, e.g., in response to a subscription.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let a = doc.state_frontiers();
    /// doc.get_text("t").insert(0, "x").unwrap();
    /// doc.commit();
    /// let b = doc.state_frontiers();
    /// let spans = doc.find_id_spans_between(&a, &b);
    /// assert!(!spans.forward.is_empty());
    /// ```
    #[inline]
    pub fn find_id_spans_between(&self, from: &Frontiers, to: &Frontiers) -> VersionVectorDiff {
        self.doc.find_id_spans_between(from, to)
    }

    /// Revert the current document state back to the target version
    ///
    /// Internally, it will generate a series of local operations that can revert the
    /// current doc to the target version. It will calculate the diff between the current
    /// state and the target state, and apply the diff to the current state.
    ///
    /// Pitfalls:
    /// - The target frontiers must be included by the document's history. If the document
    ///   is shallow and the target is before the shallow start, revert will fail.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let t = doc.get_text("text");
    /// t.insert(0, "Hello").unwrap();
    /// let v0 = doc.state_frontiers();
    /// t.insert(5, ", world").unwrap();
    /// doc.commit();
    /// doc.revert_to(&v0).unwrap();
    /// assert_eq!(t.to_string(), "Hello");
    /// ```
    #[inline]
    pub fn revert_to(&self, version: &Frontiers) -> LoroResult<()> {
        self.doc.revert_to(version)
    }

    /// Apply a diff to the current document state.
    ///
    /// Internally, it will apply the diff to the current state.
    #[inline]
    pub fn apply_diff(&self, diff: DiffBatch) -> LoroResult<()> {
        self.doc.apply_diff(diff.into())
    }

    /// Calculate the diff between two versions.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc};
    /// let doc = LoroDoc::new();
    /// let t = doc.get_text("text");
    /// let a = doc.state_frontiers();
    /// t.insert(0, "a").unwrap();
    /// let b = doc.state_frontiers();
    /// let diff = doc.diff(&a, &b).unwrap();
    /// assert!(diff.iter().next().is_some());
    /// ```
    #[inline]
    pub fn diff(&self, a: &Frontiers, b: &Frontiers) -> LoroResult<DiffBatch> {
        self.doc.diff(a, b).map(|x| x.into())
    }

    /// Check if the doc contains the target container.
    ///
    /// A root container always exists, while a normal container exists
    /// if it has ever been created on the doc.
    ///
    /// # Examples
    /// ```
    /// use loro::{LoroDoc, LoroText, LoroList, ExportMode};
    ///
    /// let doc = LoroDoc::new();
    /// doc.set_peer_id(1);
    /// let map = doc.get_map("map");
    /// map.insert_container("text", LoroText::new()).unwrap();
    /// map.insert_container("list", LoroList::new()).unwrap();
    ///
    /// // Root map container exists
    /// assert!(doc.has_container(&"cid:root-map:Map".try_into().unwrap()));
    /// // Text container exists
    /// assert!(doc.has_container(&"cid:0@1:Text".try_into().unwrap()));
    /// // List container exists
    /// assert!(doc.has_container(&"cid:1@1:List".try_into().unwrap()));
    ///
    /// let doc2 = LoroDoc::new();
    /// // Containers exist as long as the history or doc state includes them
    /// doc.detach();
    /// doc2.import(&doc.export(ExportMode::all_updates()).unwrap()).unwrap();
    /// assert!(doc2.has_container(&"cid:root-map:Map".try_into().unwrap()));
    /// assert!(doc2.has_container(&"cid:0@1:Text".try_into().unwrap()));
    /// assert!(doc2.has_container(&"cid:1@1:List".try_into().unwrap()));
    /// ```
    pub fn has_container(&self, container_id: &ContainerID) -> bool {
        self.doc.has_container(container_id)
    }

    /// Subscribe to the first commit from a peer. Operations performed on the `LoroDoc` within this callback
    /// will be merged into the current commit.
    ///
    /// Subscribe to the first commit event from each peer.
    ///
    /// The callback is triggered only once per peer when they make their first commit to the document locally.
    /// This is particularly useful for managing peer-to-user mappings or initialization logic.
    ///
    /// **Auto-unsubscription**: If the callback returns `false`, the subscription will be
    /// automatically removed, providing a convenient way to implement one-time or conditional
    /// subscriptions in Rust.
    ///
    /// # Parameters
    /// - `callback`: Function that receives `&FirstCommitFromPeerPayload` and returns `bool`
    ///   - Return `true` to keep the subscription active
    ///   - Return `false` to automatically unsubscribe
    ///
    /// # Use Cases
    /// - Initialize peer-specific data structures
    /// - Map peer IDs to user information
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    /// use std::sync::{Arc, Mutex};
    ///
    /// let doc = LoroDoc::new();
    /// doc.set_peer_id(0).unwrap();
    ///
    /// let new_peers = Arc::new(Mutex::new(Vec::new()));
    /// let peers_clone = new_peers.clone();
    /// let peer_count = Arc::new(Mutex::new(0));
    /// let count_clone = peer_count.clone();
    ///
    /// // Track first 5 new peers, then auto-unsubscribe
    /// let sub = doc.subscribe_first_commit_from_peer(Box::new(move |payload| {
    ///     peers_clone.lock().unwrap().push(payload.peer);
    ///     let mut count = count_clone.lock().unwrap();
    ///     *count += 1;
    ///     *count < 5  // Auto-unsubscribe after tracking 5 peers
    /// }));
    ///
    /// // This will trigger the callback for peer 0
    /// doc.get_text("text").insert(0, "hello").unwrap();
    /// doc.commit();
    ///
    /// // Switch to a new peer and commit - triggers callback again
    /// doc.set_peer_id(1).unwrap();
    /// doc.get_text("text").insert(0, "world").unwrap();
    /// doc.commit();
    /// ```
    pub fn subscribe_first_commit_from_peer(
        &self,
        callback: FirstCommitFromPeerCallback,
    ) -> Subscription {
        self.doc.subscribe_first_commit_from_peer(callback)
    }

    /// Subscribe to pre-commit events.
    ///
    /// The callback is triggered when changes are about to be committed but before they're
    /// applied to the OpLog. This allows you to modify commit metadata such as timestamps
    /// and messages, or perform validation before changes are finalized.
    ///
    /// **Auto-unsubscription**: If the callback returns `false`, the subscription will be
    /// automatically removed, providing a convenient way to implement one-time or conditional
    /// subscriptions in Rust.
    ///
    /// Pitfall: `commit()` can be triggered implicitly by `import`, `export`, and `checkout`.
    /// This hook still runs for those commits, which is helpful for annotating metadata
    /// even for implicit commits.
    ///
    /// # Parameters
    /// - `callback`: Function that receives `&PreCommitCallbackPayload` and returns `bool`
    ///   - Return `true` to keep the subscription active
    ///   - Return `false` to automatically unsubscribe
    /// - The payload contains:
    ///   - `change_meta`: Metadata about the commit
    ///   - `modifier`: Interface to modify commit properties
    ///
    /// # Use Cases
    /// - Add commit message prefixes or formatting
    /// - Adjust timestamps for consistent ordering
    /// - Log or audit commit operations
    /// - Implement commit validation or approval workflows
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    /// use std::sync::{Arc, Mutex};
    ///
    /// let doc = LoroDoc::new();
    /// let commit_count = Arc::new(Mutex::new(0));
    /// let count_clone = commit_count.clone();
    ///
    /// // Add timestamps and auto-unsubscribe after 5 commits
    /// let sub = doc.subscribe_pre_commit(Box::new(move |payload| {
    ///     // Add a prefix to commit messages
    ///     let new_message = format!("Auto: {}", payload.change_meta.message());
    ///     payload.modifier.set_message(&new_message);
    ///
    ///     let mut count = count_clone.lock().unwrap();
    ///     *count += 1;
    ///     *count < 5  // Auto-unsubscribe after 5 commits
    /// }));
    ///
    /// doc.get_text("text").insert(0, "hello").unwrap();
    /// doc.commit();
    /// ```
    pub fn subscribe_pre_commit(&self, callback: PreCommitCallback) -> Subscription {
        self.doc.subscribe_pre_commit(callback)
    }

    /// Delete all content from a root container and hide it from the document.
    ///
    /// When a root container is empty and hidden:
    /// - It won't show up in `get_deep_value()` results
    /// - It won't be included in document snapshots
    ///
    /// Only works on root containers (containers without parents).
    pub fn delete_root_container(&self, cid: ContainerID) {
        self.doc.delete_root_container(cid);
    }

    /// Set whether to hide empty root containers.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let map = doc.get_map("map");
    /// dbg!(doc.get_deep_value()); // {"map": {}}
    /// doc.set_hide_empty_root_containers(true);
    /// dbg!(doc.get_deep_value()); // {}
    /// ```
    pub fn set_hide_empty_root_containers(&self, hide: bool) {
        self.doc.set_hide_empty_root_containers(hide);
    }
}

/// It's used to prevent the user from implementing the trait directly.
#[allow(private_bounds)]
trait SealedTrait {}

/// The common trait for all the containers.
/// It's used internally, you can't implement it directly.
#[allow(private_bounds)]
pub trait ContainerTrait: SealedTrait {
    /// The handler of the container.
    type Handler: HandlerTrait;
    /// Get the ID of the container.
    fn id(&self) -> ContainerID;
    /// Convert the container to a [Container].
    fn to_container(&self) -> Container;
    /// Convert the container to a handler.
    fn to_handler(&self) -> Self::Handler;
    /// Convert the handler to a container.
    fn from_handler(handler: Self::Handler) -> Self;
    /// Try to convert the container to the handler.
    fn try_from_container(container: Container) -> Option<Self>
    where
        Self: Sized;
    /// Whether the container is attached to a document.
    fn is_attached(&self) -> bool;
    /// If a detached container is attached, this method will return its corresponding attached handler.
    fn get_attached(&self) -> Option<Self>
    where
        Self: Sized;
    /// Whether the container is deleted.
    fn is_deleted(&self) -> bool;
    /// Get the doc of the container.
    fn doc(&self) -> Option<LoroDoc>;
    /// Subscribe to the container.
    ///
    /// If the Container is detached, this method will return `None`.
    fn subscribe(&self, callback: Subscriber) -> Option<Subscription> {
        self.doc().map(|doc| doc.subscribe(&self.id(), callback))
    }
}

/// LoroList container. It's used to model arrays.
///
/// It can have sub containers.
///
/// Important: choose the right structure.
/// - Use `LoroList` for ordered collections where elements are appended/inserted/deleted.
/// - Use `LoroMovableList` when frequent reordering (drag-and-drop) is needed.
/// - Use `LoroMap` for keyed records and coordinates.
///
/// ```no_run
/// // Bad: coordinates in a list can diverge under concurrency
/// // let coord = doc.get_list("coord");
/// // coord.insert(0, 10).unwrap(); // x
/// // coord.insert(1, 20).unwrap(); // y
///
/// // Good: use a map for labeled fields
/// // let coord = doc.get_map("coord");
/// // coord.insert("x", 10).unwrap();
/// // coord.insert("y", 20).unwrap();
/// ```
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

    fn id(&self) -> ContainerID {
        self.handler.id()
    }

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

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }

    fn doc(&self) -> Option<LoroDoc> {
        self.handler.doc().map(|doc| {
            doc.start_auto_commit();
            LoroDoc::_new(doc)
        })
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

    /// Insert a value at the given position.
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(pos, v)
    }

    /// Delete values at the given position.
    #[inline]
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    /// Get the value at the given position.
    #[inline]
    pub fn get(&self, index: usize) -> Option<ValueOrContainer> {
        self.handler.get_(index).map(ValueOrContainer::from)
    }

    /// Get the deep value of the container.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    /// Get the shallow value of the container.
    ///
    /// This does not convert the state of sub-containers; instead, it represents them as [LoroValue::Container].
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    /// Pop the last element of the list.
    #[inline]
    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.handler.pop()
    }

    /// Push a value to the list.
    #[inline]
    pub fn push(&self, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.push(v.into())
    }

    /// Push a container to the list.
    #[inline]
    pub fn push_container<C: ContainerTrait>(&self, child: C) -> LoroResult<C> {
        let pos = self.handler.len();
        Ok(C::from_handler(
            self.handler.insert_container(pos, child.to_handler())?,
        ))
    }

    /// Iterate over the elements of the list.
    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrContainer),
    {
        self.handler.for_each(&mut |v| {
            f(ValueOrContainer::from(v));
        })
    }

    /// Get the length of the list.
    #[inline]
    pub fn len(&self) -> usize {
        self.handler.len()
    }

    /// Whether the list is empty.
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

    /// Get the cursor at the given position.
    ///
    /// Using "index" to denote cursor positions can be unstable, as positions may
    /// shift with document edits. To reliably represent a position or range within
    /// a document, it is more effective to leverage the unique ID of each item/character
    /// in a List CRDT or Text CRDT.
    ///
    /// Loro optimizes State metadata by not storing the IDs of deleted elements. This
    /// approach complicates tracking cursors since they rely on these IDs. The solution
    /// recalculates position by replaying relevant history to update stable positions
    /// accurately. To minimize the performance impact of history replay, the system
    /// updates cursor info to reference only the IDs of currently present elements,
    /// thereby reducing the need for replay.
    ///
    /// # Example
    ///
    /// ```
    /// use loro::LoroDoc;
    /// use loro_internal::cursor::Side;
    ///
    /// let doc = LoroDoc::new();
    /// let list = doc.get_list("list");
    /// list.insert(0, 0).unwrap();
    /// let cursor = list.get_cursor(0, Side::Middle).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 0);
    /// list.insert(0, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 1);
    /// list.insert(0, 0).unwrap();
    /// list.insert(0, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 3);
    /// list.insert(4, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 3);
    /// ```
    pub fn get_cursor(&self, pos: usize, side: Side) -> Option<Cursor> {
        self.handler.get_cursor(pos, side)
    }

    /// Converts the LoroList to a Vec of LoroValue.
    ///
    /// This method unwraps the internal Arc and clones the data if necessary,
    /// returning a Vec containing all the elements of the LoroList as LoroValue.
    ///
    /// # Returns
    ///
    /// A Vec<LoroValue> containing all elements of the LoroList.
    ///
    /// # Example
    ///
    /// ```
    /// use loro::{LoroDoc, LoroValue};
    ///
    /// let doc = LoroDoc::new();
    /// let list = doc.get_list("my_list");
    /// list.insert(0, 1).unwrap();
    /// list.insert(1, "hello").unwrap();
    /// list.insert(2, true).unwrap();
    ///
    /// let vec = list.to_vec();
    /// ```
    pub fn to_vec(&self) -> Vec<LoroValue> {
        self.get_value().into_list().unwrap().unwrap()
    }

    /// Delete all elements in the list.
    pub fn clear(&self) -> LoroResult<()> {
        self.handler.clear()
    }

    /// Get the ID of the list item at the given position.
    pub fn get_id_at(&self, pos: usize) -> Option<ID> {
        self.handler.get_id_at(pos)
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

    fn id(&self) -> ContainerID {
        self.handler.id()
    }

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

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }

    fn doc(&self) -> Option<LoroDoc> {
        self.handler.doc().map(|doc| {
            doc.start_auto_commit();
            LoroDoc::_new(doc)
        })
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

    /// Whether the container is attached to a document.
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Delete a key-value pair from the map.
    pub fn delete(&self, key: &str) -> LoroResult<()> {
        self.handler.delete(key)
    }

    /// Iterate over the key-value pairs of the map.
    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(&str, ValueOrContainer),
    {
        self.handler.for_each(|k, v| {
            f(k, ValueOrContainer::from(v));
        })
    }

    /// Insert a key-value pair into the map.
    ///
    /// > **Note**: When calling `map.set(key, value)` on a LoroMap, if `map.get(key)` already returns `value`,
    /// > the operation will be a no-op (no operation recorded) to avoid unnecessary updates.
    pub fn insert(&self, key: &str, value: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(key, value)
    }

    /// Get the length of the map.
    pub fn len(&self) -> usize {
        self.handler.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    /// Get the value of the map with the given key.
    pub fn get(&self, key: &str) -> Option<ValueOrContainer> {
        self.handler.get_(key).map(ValueOrContainer::from)
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
    ///
    /// Pitfalls:
    /// - Concurrently inserting different containers at the same map key on different peers
    ///   can result in one overwriting the other rather than merging. Prefer initializing
    ///   heavy/primary child containers when initializing the map.
    pub fn insert_container<C: ContainerTrait>(&self, key: &str, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler.insert_container(key, child.to_handler())?,
        ))
    }

    /// Get the shallow value of the map.
    ///
    /// It will not convert the state of sub-containers, but represent them as [LoroValue::Container].
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    /// Get the deep value of the map.
    ///
    /// It will convert the state of sub-containers into a nested JSON value.
    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    /// Get or create a container with the given key.
    ///
    /// Pitfalls:
    /// - If other peers concurrently create a different container at the same key, their state
    ///   may be overwritten. See the note in [`insert_container`].
    pub fn get_or_create_container<C: ContainerTrait>(&self, key: &str, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler
                .get_or_create_container(key, child.to_handler())?,
        ))
    }

    /// Delete all key-value pairs in the map.
    pub fn clear(&self) -> LoroResult<()> {
        self.handler.clear()
    }

    /// Get the keys of the map.
    pub fn keys(&self) -> impl Iterator<Item = InternalString> + '_ {
        self.handler.keys()
    }

    /// Get the values of the map.
    pub fn values(&self) -> impl Iterator<Item = ValueOrContainer> + '_ {
        self.handler.values().map(ValueOrContainer::from)
    }

    /// Get the peer id of the last editor on the given entry
    pub fn get_last_editor(&self, key: &str) -> Option<PeerID> {
        self.handler.get_last_editor(key)
    }
}

impl Default for LoroMap {
    fn default() -> Self {
        Self::new()
    }
}

/// LoroText container. It's used to model plaintext/richtext.
///
/// Indexing and lengths:
/// - Rust APIs default to Unicode scalar positions for `insert`/`delete` and `slice`.
/// - For byte-based integration, use `insert_utf8`/`delete_utf8`.
/// - You can inspect `len_unicode`, `len_utf8`, and `len_utf16` depending on your needs.
///
/// # Example (emoji)
/// ```
/// use loro::LoroDoc;
/// let doc = LoroDoc::new();
/// let text = doc.get_text("text");
/// text.insert(0, "Hello 😀 World").unwrap();
/// assert_eq!(text.len_unicode(), 13); // visible characters
/// assert!(text.len_utf16() >= text.len_unicode()); // emoji may count as 2 in UTF-16
/// // Delete the emoji safely by Unicode indices
/// let start = 6; // after "Hello "
/// text.delete(start, 1).unwrap();
/// assert_eq!(text.to_string(), "Hello  World");
/// ```
#[derive(Clone, Debug)]
pub struct LoroText {
    handler: InnerTextHandler,
}

impl SealedTrait for LoroText {}
impl ContainerTrait for LoroText {
    type Handler = InnerTextHandler;

    fn id(&self) -> ContainerID {
        self.handler.id()
    }

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

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }

    fn doc(&self) -> Option<LoroDoc> {
        self.handler.doc().map(|doc| {
            doc.start_auto_commit();
            LoroDoc::_new(doc)
        })
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

    /// Iterate over contiguous text chunks.
    ///
    /// The callback function will be called for each contiguous text segment (internal span),
    /// not necessarily a single character. If you need per-character iteration, iterate the
    /// returned `&str` within the callback.
    /// If the callback returns `false`, the iteration will stop.
    ///
    /// Limitation: you cannot access or alter the doc state when iterating.
    /// If you need to access or alter the doc state, please use `to_string` instead.
    pub fn iter(&self, callback: impl FnMut(&str) -> bool) {
        self.handler.iter(callback);
    }

    /// Insert a string at the given unicode position.
    pub fn insert(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.handler.insert_unicode(pos, s)
    }

    /// Insert a string at the given utf-8 position.
    pub fn insert_utf8(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.handler.insert_utf8(pos, s)
    }

    /// Delete a range of text at the given unicode position with unicode length.
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete_unicode(pos, len)
    }

    /// Delete a range of text at the given utf-8 position with utf-8 length.
    pub fn delete_utf8(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete_utf8(pos, len)
    }

    /// Get a string slice at the given Unicode range
    pub fn slice(&self, start_index: usize, end_index: usize) -> LoroResult<String> {
        self.handler.slice(start_index, end_index)
    }

    /// Get the characters at given unicode position.
    pub fn char_at(&self, pos: usize) -> LoroResult<char> {
        self.handler.char_at(pos)
    }

    /// Delete specified character and insert string at the same position at given unicode position.
    pub fn splice(&self, pos: usize, len: usize, s: &str) -> LoroResult<String> {
        self.handler.splice(pos, len, s)
    }

    /// Whether the text container is empty.
    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    /// Get the length of the text container in UTF-8.
    pub fn len_utf8(&self) -> usize {
        self.handler.len_utf8()
    }

    /// Get the length of the text container in Unicode.
    pub fn len_unicode(&self) -> usize {
        self.handler.len_unicode()
    }

    /// Get the length of the text container in UTF-16.
    pub fn len_utf16(&self) -> usize {
        self.handler.len_utf16()
    }

    /// Update the current text based on the provided text.
    ///
    /// It will calculate the minimal difference and apply it to the current text.
    /// It uses Myers' diff algorithm to compute the optimal difference.
    ///
    /// This could take a long time for large texts (e.g. > 50_000 characters).
    /// In that case, you should use `updateByLine` instead.
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello").unwrap();
    /// text.update("Hello World", Default::default()).unwrap();
    /// assert_eq!(text.to_string(), "Hello World");
    /// ```
    ///
    pub fn update(&self, text: &str, options: UpdateOptions) -> Result<(), UpdateTimeoutError> {
        self.handler.update(text, options)
    }

    /// Update the current text based on the provided text.
    ///
    /// This update calculation is line-based, which will be more efficient but less precise.
    pub fn update_by_line(
        &self,
        text: &str,
        options: UpdateOptions,
    ) -> Result<(), UpdateTimeoutError> {
        self.handler.update_by_line(text, options)
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
    /// use loro::{LoroDoc, ToJson, ExpandType, TextDelta};
    /// use serde_json::json;
    /// use rustc_hash::FxHashMap;
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello world!").unwrap();
    /// text.mark(0..5, "bold", true).unwrap();
    /// assert_eq!(
    ///     text.to_delta(),
    ///     vec![
    ///         TextDelta::Insert {
    ///             insert: "Hello".to_string(),
    ///             attributes: Some(FxHashMap::from_iter([("bold".to_string(), true.into())])),
    ///         },
    ///         TextDelta::Insert {
    ///             insert: " world!".to_string(),
    ///             attributes: None,
    ///         },
    ///     ]
    /// );
    /// text.unmark(3..5, "bold").unwrap();
    /// assert_eq!(
    ///     text.to_delta(),
    ///     vec![
    ///         TextDelta::Insert {
    ///             insert: "Hel".to_string(),
    ///             attributes: Some(FxHashMap::from_iter([("bold".to_string(), true.into())])),
    ///         },
    ///         TextDelta::Insert {
    ///             insert: "lo world!".to_string(),
    ///             attributes: None,
    ///         },
    ///     ]
    /// );
    /// ```
    pub fn to_delta(&self) -> Vec<TextDelta> {
        let delta = self.handler.get_richtext_value().into_list().unwrap();
        delta
            .iter()
            .map(|x| {
                let map = x.as_map().unwrap();
                let insert = map.get("insert").unwrap().as_string().unwrap().to_string();
                let attributes = map
                    .get("attributes")
                    .map(|v| v.as_map().unwrap().deref().clone());
                TextDelta::Insert { insert, attributes }
            })
            .collect()
    }

    /// Get the rich text value in [Delta](https://quilljs.com/docs/delta/) format.
    ///
    /// # Example
    /// ```
    /// # use loro::{LoroDoc, ToJson, ExpandType, TextDelta};
    /// # use serde_json::json;
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello world!").unwrap();
    /// text.mark(0..5, "bold", true).unwrap();
    /// assert_eq!(
    ///     text.get_richtext_value().to_json_value(),
    ///     json!([
    ///         { "insert": "Hello", "attributes": {"bold": true} },
    ///         { "insert": " world!" },
    ///     ])
    /// );
    /// text.unmark(3..5, "bold").unwrap();
    /// assert_eq!(
    ///     text.get_richtext_value().to_json_value(),
    ///     json!([
    ///         { "insert": "Hel", "attributes": {"bold": true} },
    ///         { "insert": "lo world!" },
    ///    ])
    /// );
    /// ```
    pub fn get_richtext_value(&self) -> LoroValue {
        self.handler.get_richtext_value()
    }

    /// Get the text content of the text container.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.handler.to_string()
    }

    /// Get the cursor at the given position in the given Unicode position.
    ///
    /// Using "index" to denote cursor positions can be unstable, as positions may
    /// shift with document edits. To reliably represent a position or range within
    /// a document, it is more effective to leverage the unique ID of each item/character
    /// in a List CRDT or Text CRDT.
    ///
    /// Loro optimizes State metadata by not storing the IDs of deleted elements. This
    /// approach complicates tracking cursors since they rely on these IDs. The solution
    /// recalculates position by replaying relevant history to update stable positions
    /// accurately. To minimize the performance impact of history replay, the system
    /// updates cursor info to reference only the IDs of currently present elements,
    /// thereby reducing the need for replay.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, ToJson};
    /// let doc = LoroDoc::new();
    /// let text = &doc.get_text("text");
    /// text.insert(0, "01234").unwrap();
    /// let pos = text.get_cursor(5, Default::default()).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    /// text.insert(0, "01234").unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 10);
    /// text.delete(0, 10).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 0);
    /// text.insert(0, "01234").unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    /// ```
    pub fn get_cursor(&self, pos: usize, side: Side) -> Option<Cursor> {
        self.handler.get_cursor(pos, side)
    }

    /// Whether the text container is deleted.
    pub fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }

    /// Push a string to the end of the text container.
    pub fn push_str(&self, s: &str) -> LoroResult<()> {
        self.handler.push_str(s)
    }

    /// Get the editor of the text at the given position.
    ///
    /// Returns `None` if the position is out of bounds or attribution is unavailable.
    ///
    /// # Example
    /// ```
    /// use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let t = doc.get_text("t");
    /// t.insert(0, "hi").unwrap();
    /// let who = t.get_editor_at_unicode_pos(0);
    /// assert!(who.is_some());
    /// ```
    pub fn get_editor_at_unicode_pos(&self, pos: usize) -> Option<PeerID> {
        self.handler
            .get_cursor(pos, Side::Middle)
            .map(|x| x.id.unwrap().peer)
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
///
/// Learn more at https://loro.dev/docs/tutorial/tree
#[derive(Clone, Debug)]
pub struct LoroTree {
    handler: InnerTreeHandler,
}

impl SealedTrait for LoroTree {}
impl ContainerTrait for LoroTree {
    type Handler = InnerTreeHandler;

    fn id(&self) -> ContainerID {
        self.handler.id()
    }

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

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }
    fn doc(&self) -> Option<LoroDoc> {
        self.handler.doc().map(|doc| {
            doc.start_auto_commit();
            LoroDoc::_new(doc)
        })
    }
}

/// A tree node in the [LoroTree].
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// ID of the tree node.
    pub id: TreeID,
    /// ID of the parent tree node.
    /// If the node is deleted this value is TreeParentId::Deleted.
    /// If you checkout to a version before the node is created, this value is TreeParentId::Unexist.
    pub parent: TreeParentId,
    /// Fraction index of the node
    pub fractional_index: FractionalIndex,
    /// The current index of the node in its parent's children list.
    pub index: usize,
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
    pub fn create<T: Into<TreeParentId>>(&self, parent: T) -> LoroResult<TreeID> {
        self.handler.create(parent.into())
    }

    /// Get the root nodes of the forest.
    pub fn roots(&self) -> Vec<TreeID> {
        self.handler.roots()
    }

    /// Create a new tree node at the given index and return the [`TreeID`].
    ///
    /// If the `parent` is `None`, the created node is the root of a tree.
    /// If the `index` is greater than the number of children of the parent, error will be returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// // enable generate fractional index
    /// tree.enable_fractional_index(0);
    /// // create a root
    /// let root = tree.create(None).unwrap();
    /// // create a new child at index 0
    /// let child = tree.create_at(root, 0).unwrap();
    /// ```
    pub fn create_at<T: Into<TreeParentId>>(&self, parent: T, index: usize) -> LoroResult<TreeID> {
        if !self.handler.is_fractional_index_enabled() {
            return Err(LoroTreeError::FractionalIndexNotEnabled.into());
        }
        self.handler.create_at(parent.into(), index)
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
    pub fn mov<T: Into<TreeParentId>>(&self, target: TreeID, parent: T) -> LoroResult<()> {
        self.handler.mov(target, parent.into())
    }

    /// Move the `target` node to be a child of the `parent` node at the given index.
    /// If the `parent` is `None`, the `target` node will be a root.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// // enable generate fractional index
    /// tree.enable_fractional_index(0);
    /// let root = tree.create(None).unwrap();
    /// let root2 = tree.create(None).unwrap();
    /// // move `root2` to be a child of `root` at index 0.
    /// tree.mov_to(root2, root, 0).unwrap();
    /// ```
    pub fn mov_to<T: Into<TreeParentId>>(
        &self,
        target: TreeID,
        parent: T,
        to: usize,
    ) -> LoroResult<()> {
        if !self.handler.is_fractional_index_enabled() {
            return Err(LoroTreeError::FractionalIndexNotEnabled.into());
        }
        self.handler.move_to(target, parent.into(), to)
    }

    /// Move the `target` node to be a child after the `after` node with the same parent.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// // enable generate fractional index
    /// tree.enable_fractional_index(0);
    /// let root = tree.create(None).unwrap();
    /// let root2 = tree.create(None).unwrap();
    /// // move `root` to be a child after `root2`.
    /// tree.mov_after(root, root2).unwrap();
    /// ```
    pub fn mov_after(&self, target: TreeID, after: TreeID) -> LoroResult<()> {
        if !self.handler.is_fractional_index_enabled() {
            return Err(LoroTreeError::FractionalIndexNotEnabled.into());
        }
        self.handler.mov_after(target, after)
    }

    /// Move the `target` node to be a child before the `before` node with the same parent.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// // enable generate fractional index
    /// tree.enable_fractional_index(0);
    /// let root = tree.create(None).unwrap();
    /// let root2 = tree.create(None).unwrap();
    /// // move `root` to be a child before `root2`.
    /// tree.mov_before(root, root2).unwrap();
    /// ```
    pub fn mov_before(&self, target: TreeID, before: TreeID) -> LoroResult<()> {
        if !self.handler.is_fractional_index_enabled() {
            return Err(LoroTreeError::FractionalIndexNotEnabled.into());
        }
        self.handler.mov_before(target, before)
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
    pub fn parent(&self, target: TreeID) -> Option<TreeParentId> {
        self.handler.get_node_parent(&target)
    }

    /// Return whether target node exists. including deleted node.
    pub fn contains(&self, target: TreeID) -> bool {
        self.handler.contains(target)
    }

    /// Return whether target node is deleted.
    ///
    /// # Errors
    ///
    /// - If the target node does not exist, return `LoroTreeError::TreeNodeNotExist`.
    pub fn is_node_deleted(&self, target: &TreeID) -> LoroResult<bool> {
        self.handler.is_node_deleted(target)
    }

    /// Return all nodes, including deleted nodes
    pub fn nodes(&self) -> Vec<TreeID> {
        self.handler.nodes()
    }

    /// Return all nodes, if `with_deleted` is true, the deleted nodes will be included.
    pub fn get_nodes(&self, with_deleted: bool) -> Vec<TreeNode> {
        let mut ans = self.handler.get_nodes_under(TreeParentId::Root);
        if with_deleted {
            ans.extend(self.handler.get_nodes_under(TreeParentId::Deleted));
        }
        ans.into_iter()
            .map(|x| TreeNode {
                id: x.id,
                parent: x.parent,
                fractional_index: x.fractional_index,
                index: x.index,
            })
            .collect()
    }

    /// Return all children of the target node.
    ///
    /// If the parent node does not exist, return `None`.
    pub fn children<T: Into<TreeParentId>>(&self, parent: T) -> Option<Vec<TreeID>> {
        self.handler.children(&parent.into())
    }

    /// Return the number of children of the target node.
    pub fn children_num<T: Into<TreeParentId>>(&self, parent: T) -> Option<usize> {
        let parent: TreeParentId = parent.into();
        self.handler.children_num(&parent)
    }

    /// Return the fractional index of the target node with hex format.
    pub fn fractional_index(&self, target: TreeID) -> Option<String> {
        self.handler
            .get_position_by_tree_id(&target)
            .map(|x| x.to_string())
    }

    /// Return the hierarchy array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use [TreeHandler::get_value_with_meta()].
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    /// Return the hierarchy array of the forest, each node is with metadata.
    pub fn get_value_with_meta(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    // This method is used for testing only.
    #[doc(hidden)]
    #[allow(non_snake_case)]
    pub fn __internal__next_tree_id(&self) -> TreeID {
        self.handler.__internal__next_tree_id()
    }

    /// Whether the fractional index is enabled.
    pub fn is_fractional_index_enabled(&self) -> bool {
        self.handler.is_fractional_index_enabled()
    }

    /// Enable fractional index for Tree Position.
    ///
    /// The jitter is used to avoid conflicts when multiple users are creating the node at the same position.
    /// value 0 is default, which means no jitter, any value larger than 0 will enable jitter.
    ///
    /// Generally speaking, jitter will affect the growth rate of document size.
    /// [Read more about it](https://www.loro.dev/blog/movable-tree#implementation-and-encoding-size)
    #[inline]
    pub fn enable_fractional_index(&self, jitter: u8) {
        self.handler.enable_fractional_index(jitter);
    }

    /// Disable the fractional index generation when you don't need the Tree's siblings to be sorted.
    /// The fractional index will always be set to the same default value 0.
    ///
    /// After calling this, you cannot use `tree.moveTo()`, `tree.moveBefore()`, `tree.moveAfter()`,
    /// and `tree.createAt()`.
    #[inline]
    pub fn disable_fractional_index(&self) {
        self.handler.disable_fractional_index();
    }

    /// Whether the tree is empty.
    ///
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.handler.is_empty()
    }

    /// Get the last move id of the target node.
    pub fn get_last_move_id(&self, target: &TreeID) -> Option<ID> {
        self.handler.get_last_move_id(target)
    }
}

impl Default for LoroTree {
    fn default() -> Self {
        Self::new()
    }
}

/// [LoroMovableList container](https://loro.dev/docs/tutorial/list)
///
/// It is used to model movable ordered lists.
///
/// Using a combination of insert and delete operations, one can simulate set and move
/// operations on a List. However, this approach fails in concurrent editing scenarios.
/// For example, if the same element is set or moved concurrently, the simulation would
/// result in the deletion of the original element and the insertion of two new elements,
/// which does not meet expectations.
#[derive(Clone, Debug)]
pub struct LoroMovableList {
    handler: InnerMovableListHandler,
}

impl SealedTrait for LoroMovableList {}
impl ContainerTrait for LoroMovableList {
    type Handler = InnerMovableListHandler;

    fn id(&self) -> ContainerID {
        self.handler.id()
    }

    fn to_container(&self) -> Container {
        Container::MovableList(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn try_from_container(container: Container) -> Option<Self>
    where
        Self: Sized,
    {
        match container {
            Container::MovableList(x) => Some(x),
            _ => None,
        }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self>
    where
        Self: Sized,
    {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }

    fn doc(&self) -> Option<LoroDoc> {
        self.handler.doc().map(|doc| {
            doc.start_auto_commit();
            LoroDoc::_new(doc)
        })
    }
}

impl LoroMovableList {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new() -> LoroMovableList {
        Self {
            handler: InnerMovableListHandler::new_detached(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Insert a value at the given position.
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(pos, v)
    }

    /// Delete the value at the given position.
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    /// Get the value at the given position.
    pub fn get(&self, index: usize) -> Option<ValueOrContainer> {
        self.handler.get_(index).map(ValueOrContainer::from)
    }

    /// Get the length of the list.
    pub fn len(&self) -> usize {
        self.handler.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the shallow value of the list.
    ///
    /// It will not convert the state of sub-containers, but represent them as [LoroValue::Container].
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
    }

    /// Get the deep value of the list.
    ///
    /// It will convert the state of sub-containers into a nested JSON value.
    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value()
    }

    /// Pop the last element of the list.
    pub fn pop(&self) -> LoroResult<Option<ValueOrContainer>> {
        let ans = self.handler.pop_()?.map(ValueOrContainer::from);
        Ok(ans)
    }

    /// Push a value to the end of the list.
    pub fn push(&self, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.push(v.into())
    }

    /// Push a container to the end of the list.
    pub fn push_container<C: ContainerTrait>(&self, child: C) -> LoroResult<C> {
        let pos = self.handler.len();
        Ok(C::from_handler(
            self.handler.insert_container(pos, child.to_handler())?,
        ))
    }

    /// Set the value at the given position.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ToJson};
    /// use serde_json::json;
    /// let doc = LoroDoc::new();
    /// let ml = doc.get_movable_list("ml");
    /// ml.insert(0, "a").unwrap();
    /// ml.set(0, "b").unwrap();
    /// assert_eq!(ml.get_deep_value().to_json_value(), json!(["b"]));
    /// ```
    pub fn set(&self, pos: usize, value: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.set(pos, value.into())
    }

    /// Move the value at the given position to the given position.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ToJson};
    /// use serde_json::json;
    /// let doc = LoroDoc::new();
    /// let ml = doc.get_movable_list("ml");
    /// ml.insert(0, "a").unwrap();
    /// ml.insert(1, "b").unwrap();
    /// ml.insert(2, "c").unwrap();
    /// ml.mov(0, 2).unwrap();
    /// assert_eq!(ml.get_deep_value().to_json_value(), json!(["b","c","a"]));
    /// ```
    pub fn mov(&self, from: usize, to: usize) -> LoroResult<()> {
        self.handler.mov(from, to)
    }

    /// Insert a container at the given position.
    pub fn insert_container<C: ContainerTrait>(&self, pos: usize, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler.insert_container(pos, child.to_handler())?,
        ))
    }

    /// Set the container at the given position.
    pub fn set_container<C: ContainerTrait>(&self, pos: usize, child: C) -> LoroResult<C> {
        Ok(C::from_handler(
            self.handler.set_container(pos, child.to_handler())?,
        ))
    }

    /// Log the internal state of the list.
    pub fn log_internal_state(&self) {
        info!(
            "movable_list internal state: {}",
            self.handler.log_internal_state()
        )
    }

    /// Get the cursor at the given position.
    ///
    /// Using "index" to denote cursor positions can be unstable, as positions may
    /// shift with document edits. To reliably represent a position or range within
    /// a document, it is more effective to leverage the unique ID of each item/character
    /// in a List CRDT or Text CRDT.
    ///
    /// Loro optimizes State metadata by not storing the IDs of deleted elements. This
    /// approach complicates tracking cursors since they rely on these IDs. The solution
    /// recalculates position by replaying relevant history to update stable positions
    /// accurately. To minimize the performance impact of history replay, the system
    /// updates cursor info to reference only the IDs of currently present elements,
    /// thereby reducing the need for replay.
    ///
    /// # Example
    ///
    /// ```
    /// use loro::LoroDoc;
    /// use loro_internal::cursor::Side;
    ///
    /// let doc = LoroDoc::new();
    /// let list = doc.get_movable_list("list");
    /// list.insert(0, 0).unwrap();
    /// let cursor = list.get_cursor(0, Side::Middle).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 0);
    /// list.insert(0, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 1);
    /// list.insert(0, 0).unwrap();
    /// list.insert(0, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 3);
    /// list.insert(4, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 3);
    /// ```
    pub fn get_cursor(&self, pos: usize, side: Side) -> Option<Cursor> {
        self.handler.get_cursor(pos, side)
    }

    /// Get the elements of the list as a vector of LoroValues.
    ///
    /// This method returns a vector containing all the elements in the list as LoroValues.
    /// It provides a convenient way to access the entire contents of the LoroMovableList
    /// as a standard Rust vector.
    ///
    /// # Returns
    ///
    /// A `Vec<LoroValue>` containing all elements of the list.
    ///
    /// # Example
    ///
    /// ```
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let list = doc.get_movable_list("mylist");
    /// list.insert(0, 1).unwrap();
    /// list.insert(1, "hello").unwrap();
    /// list.insert(2, true).unwrap();
    ///
    /// let vec = list.to_vec();
    /// assert_eq!(vec.len(), 3);
    /// assert_eq!(vec[0], 1.into());
    /// assert_eq!(vec[1], "hello".into());
    /// assert_eq!(vec[2], true.into());
    /// ```
    pub fn to_vec(&self) -> Vec<LoroValue> {
        self.get_value().into_list().unwrap().unwrap()
    }

    /// Delete all elements in the list.
    pub fn clear(&self) -> LoroResult<()> {
        self.handler.clear()
    }

    /// Iterate over the elements of the list.
    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrContainer),
    {
        self.handler.for_each(&mut |v| {
            f(ValueOrContainer::from(v));
        })
    }

    /// Get the creator of the list item at the given position.
    pub fn get_creator_at(&self, pos: usize) -> Option<PeerID> {
        self.handler.get_creator_at(pos)
    }

    /// Get the last mover of the list item at the given position.
    pub fn get_last_mover_at(&self, pos: usize) -> Option<PeerID> {
        self.handler.get_last_mover_at(pos)
    }

    /// Get the last editor of the list item at the given position.
    pub fn get_last_editor_at(&self, pos: usize) -> Option<PeerID> {
        self.handler.get_last_editor_at(pos)
    }
}

impl Default for LoroMovableList {
    fn default() -> Self {
        Self::new()
    }
}

/// Unknown container.
#[derive(Clone, Debug)]
pub struct LoroUnknown {
    handler: InnerUnknownHandler,
}

impl SealedTrait for LoroUnknown {}
impl ContainerTrait for LoroUnknown {
    type Handler = InnerUnknownHandler;

    fn id(&self) -> ContainerID {
        self.handler.id()
    }

    fn to_container(&self) -> Container {
        Container::Unknown(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn try_from_container(container: Container) -> Option<Self>
    where
        Self: Sized,
    {
        match container {
            Container::Unknown(x) => Some(x),
            _ => None,
        }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self>
    where
        Self: Sized,
    {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }

    fn doc(&self) -> Option<LoroDoc> {
        self.handler.doc().map(|doc| {
            doc.start_auto_commit();
            LoroDoc::_new(doc)
        })
    }
}

use enum_as_inner::EnumAsInner;
#[cfg(feature = "jsonpath")]
use loro_internal::jsonpath::jsonpath::JsonPathError;

/// All the CRDT containers supported by Loro.
#[derive(Clone, Debug, EnumAsInner)]
pub enum Container {
    /// [LoroList container](https://loro.dev/docs/tutorial/list)
    List(LoroList),
    /// [LoroMap container](https://loro.dev/docs/tutorial/map)
    Map(LoroMap),
    /// [LoroText container](https://loro.dev/docs/tutorial/text)
    Text(LoroText),
    /// [LoroTree container]
    Tree(LoroTree),
    /// [LoroMovableList container](https://loro.dev/docs/tutorial/list)
    MovableList(LoroMovableList),
    #[cfg(feature = "counter")]
    /// [LoroCounter container]
    Counter(counter::LoroCounter),
    /// Unknown container
    Unknown(LoroUnknown),
}

impl SealedTrait for Container {}
impl ContainerTrait for Container {
    type Handler = loro_internal::handler::Handler;

    fn id(&self) -> ContainerID {
        match self {
            Container::List(x) => x.id(),
            Container::Map(x) => x.id(),
            Container::Text(x) => x.id(),
            Container::Tree(x) => x.id(),
            Container::MovableList(x) => x.id(),
            #[cfg(feature = "counter")]
            Container::Counter(x) => x.id(),
            Container::Unknown(x) => x.id(),
        }
    }

    fn to_container(&self) -> Container {
        self.clone()
    }

    fn to_handler(&self) -> Self::Handler {
        match self {
            Container::List(x) => Self::Handler::List(x.to_handler()),
            Container::Map(x) => Self::Handler::Map(x.to_handler()),
            Container::Text(x) => Self::Handler::Text(x.to_handler()),
            Container::Tree(x) => Self::Handler::Tree(x.to_handler()),
            Container::MovableList(x) => Self::Handler::MovableList(x.to_handler()),
            #[cfg(feature = "counter")]
            Container::Counter(x) => Self::Handler::Counter(x.to_handler()),
            Container::Unknown(x) => Self::Handler::Unknown(x.to_handler()),
        }
    }

    fn from_handler(handler: Self::Handler) -> Self {
        match handler {
            InnerHandler::Text(x) => Container::Text(LoroText { handler: x }),
            InnerHandler::Map(x) => Container::Map(LoroMap { handler: x }),
            InnerHandler::List(x) => Container::List(LoroList { handler: x }),
            InnerHandler::MovableList(x) => Container::MovableList(LoroMovableList { handler: x }),
            InnerHandler::Tree(x) => Container::Tree(LoroTree { handler: x }),
            #[cfg(feature = "counter")]
            InnerHandler::Counter(x) => Container::Counter(counter::LoroCounter { handler: x }),
            InnerHandler::Unknown(x) => Container::Unknown(LoroUnknown { handler: x }),
        }
    }

    fn is_attached(&self) -> bool {
        match self {
            Container::List(x) => x.is_attached(),
            Container::Map(x) => x.is_attached(),
            Container::Text(x) => x.is_attached(),
            Container::Tree(x) => x.is_attached(),
            Container::MovableList(x) => x.is_attached(),
            #[cfg(feature = "counter")]
            Container::Counter(x) => x.is_attached(),
            Container::Unknown(x) => x.is_attached(),
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match self {
            Container::List(x) => x.get_attached().map(Container::List),
            Container::MovableList(x) => x.get_attached().map(Container::MovableList),
            Container::Map(x) => x.get_attached().map(Container::Map),
            Container::Text(x) => x.get_attached().map(Container::Text),
            Container::Tree(x) => x.get_attached().map(Container::Tree),
            #[cfg(feature = "counter")]
            Container::Counter(x) => x.get_attached().map(Container::Counter),
            Container::Unknown(x) => x.get_attached().map(Container::Unknown),
        }
    }

    fn try_from_container(container: Container) -> Option<Self>
    where
        Self: Sized,
    {
        Some(container)
    }

    fn is_deleted(&self) -> bool {
        match self {
            Container::List(x) => x.is_deleted(),
            Container::Map(x) => x.is_deleted(),
            Container::Text(x) => x.is_deleted(),
            Container::Tree(x) => x.is_deleted(),
            Container::MovableList(x) => x.is_deleted(),
            #[cfg(feature = "counter")]
            Container::Counter(x) => x.is_deleted(),
            Container::Unknown(x) => x.is_deleted(),
        }
    }
    fn doc(&self) -> Option<LoroDoc> {
        match self {
            Container::List(x) => x.doc(),
            Container::Map(x) => x.doc(),
            Container::Text(x) => x.doc(),
            Container::Tree(x) => x.doc(),
            Container::MovableList(x) => x.doc(),
            #[cfg(feature = "counter")]
            Container::Counter(x) => x.doc(),
            Container::Unknown(x) => x.doc(),
        }
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
            ContainerType::MovableList => Container::MovableList(LoroMovableList::new()),
            ContainerType::Map => Container::Map(LoroMap::new()),
            ContainerType::Text => Container::Text(LoroText::new()),
            ContainerType::Tree => Container::Tree(LoroTree::new()),
            #[cfg(feature = "counter")]
            ContainerType::Counter => Container::Counter(counter::LoroCounter::new()),
            ContainerType::Unknown(_) => unreachable!(),
        }
    }

    /// Get the type of the container.
    pub fn get_type(&self) -> ContainerType {
        match self {
            Container::List(_) => ContainerType::List,
            Container::MovableList(_) => ContainerType::MovableList,
            Container::Map(_) => ContainerType::Map,
            Container::Text(_) => ContainerType::Text,
            Container::Tree(_) => ContainerType::Tree,
            #[cfg(feature = "counter")]
            Container::Counter(_) => ContainerType::Counter,
            Container::Unknown(x) => x.handler.id().container_type(),
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
            InnerHandler::MovableList(x) => Container::MovableList(LoroMovableList { handler: x }),
            #[cfg(feature = "counter")]
            InnerHandler::Counter(x) => Container::Counter(counter::LoroCounter { handler: x }),
            InnerHandler::Unknown(x) => Container::Unknown(LoroUnknown { handler: x }),
        }
    }
}

/// It's a type that can be either a value or a container.
#[derive(Debug, Clone, EnumAsInner)]
pub enum ValueOrContainer {
    /// A value.
    Value(LoroValue),
    /// A container.
    Container(Container),
}

impl ValueOrContainer {
    /// Get the deep value of the value or container.
    pub fn get_deep_value(&self) -> LoroValue {
        match self {
            ValueOrContainer::Value(v) => v.clone(),
            ValueOrContainer::Container(c) => match c {
                Container::List(c) => c.get_deep_value(),
                Container::Map(c) => c.get_deep_value(),
                Container::Text(c) => c.to_string().into(),
                Container::Tree(c) => c.get_value(),
                Container::MovableList(c) => c.get_deep_value(),
                #[cfg(feature = "counter")]
                Container::Counter(c) => c.get_value().into(),
                Container::Unknown(_) => LoroValue::Null,
            },
        }
    }

    pub(crate) fn into_value_or_handler(self) -> ValueOrHandler {
        match self {
            ValueOrContainer::Value(v) => ValueOrHandler::Value(v),
            ValueOrContainer::Container(c) => ValueOrHandler::Handler(c.to_handler()),
        }
    }
}

/// UndoManager can be used to undo and redo the changes made to the document with a certain peer.
///
/// Notes & pitfalls:
/// - Local-only: undo/redo affects only local operations from the bound peer; it does not revert
///   remote edits. For global rollback, use time travel (`checkout`/`revert_to`).
/// - Peer identity: keep the `peer_id` stable while an `UndoManager` is in use. Changing peer IDs
///   can disrupt undo grouping/semantics.
/// - Grouping: you may want to tune the merge interval and exclude origins to group related edits.
#[derive(Debug)]
#[repr(transparent)]
pub struct UndoManager(InnerUndoManager);

impl UndoManager {
    /// Create a new UndoManager.
    pub fn new(doc: &LoroDoc) -> Self {
        let inner = InnerUndoManager::new(&doc.doc);
        inner.set_max_undo_steps(100);
        Self(inner)
    }

    /// Undo the last change made by the peer.
    pub fn undo(&mut self) -> LoroResult<bool> {
        self.0.undo()
    }

    /// Redo the last change made by the peer.
    pub fn redo(&mut self) -> LoroResult<bool> {
        self.0.redo()
    }

    /// Record a new checkpoint.
    pub fn record_new_checkpoint(&mut self) -> LoroResult<()> {
        self.0.record_new_checkpoint()
    }

    /// Whether the undo manager can undo.
    pub fn can_undo(&self) -> bool {
        self.0.can_undo()
    }

    /// Whether the undo manager can redo.
    pub fn can_redo(&self) -> bool {
        self.0.can_redo()
    }

    /// How many times the undo manager can undo.
    pub fn undo_count(&self) -> usize {
        self.0.undo_count()
    }

    /// How many times the undo manager can redo.
    pub fn redo_count(&self) -> usize {
        self.0.redo_count()
    }

    /// If a local event's origin matches the given prefix, it will not be recorded in the
    /// undo stack.
    pub fn add_exclude_origin_prefix(&mut self, prefix: &str) {
        self.0.add_exclude_origin_prefix(prefix)
    }

    /// Set the maximum number of undo steps. The default value is 100.
    pub fn set_max_undo_steps(&mut self, size: usize) {
        self.0.set_max_undo_steps(size)
    }

    /// Set the merge interval in ms. The default value is 0, which means no merge.
    pub fn set_merge_interval(&mut self, interval: i64) {
        self.0.set_merge_interval(interval)
    }

    /// Set the listener for push events.
    /// The listener will be called when a new undo/redo item is pushed into the stack.
    pub fn set_on_push(&mut self, on_push: Option<OnPush>) {
        if let Some(on_push) = on_push {
            self.0.set_on_push(Some(Box::new(move |u, c, e| {
                on_push(u, c, e.map(|x| x.into()))
            })));
        } else {
            self.0.set_on_push(None);
        }
    }

    /// Set the listener for pop events.
    /// The listener will be called when an undo/redo item is popped from the stack.
    pub fn set_on_pop(&mut self, on_pop: Option<OnPop>) {
        self.0.set_on_pop(on_pop);
    }

    /// Clear the undo stack and the redo stack
    pub fn clear(&self) {
        self.0.clear();
    }

    /// Will start a new group of changes, all subsequent changes will be merged
    /// into a new item on the undo stack. If we receive remote changes, we determine
    /// wether or not they are conflicting. If the remote changes are conflicting
    /// we split the undo item and close the group. If there are no conflict
    /// in changed container ids we continue the group merge.
    pub fn group_start(&mut self) -> LoroResult<()> {
        self.0.group_start()
    }

    /// Ends the current group, calling UndoManager::undo() after this will
    /// undo all changes that occurred during the group.
    pub fn group_end(&mut self) {
        self.0.group_end();
    }

    /// Get the peer id of the undo manager.
    pub fn peer(&self) -> PeerID {
        self.0.peer()
    }

    /// Get the metadata of the top undo stack item, if any.
    pub fn top_undo_meta(&self) -> Option<UndoItemMeta> {
        self.0.top_undo_meta()
    }

    /// Get the metadata of the top redo stack item, if any.
    pub fn top_redo_meta(&self) -> Option<UndoItemMeta> {
        self.0.top_redo_meta()
    }

    /// Get the value associated with the top undo stack item, if any.
    pub fn top_undo_value(&self) -> Option<LoroValue> {
        self.0.top_undo_value()
    }

    /// Get the value associated with the top redo stack item, if any.
    pub fn top_redo_value(&self) -> Option<LoroValue> {
        self.0.top_redo_value()
    }
}
/// When a undo/redo item is pushed, the undo manager will call the on_push callback to get the meta data of the undo item.
/// The returned cursors will be recorded for a new pushed undo item.
pub type OnPush =
    Box<dyn for<'a> Fn(UndoOrRedo, CounterSpan, Option<DiffEvent>) -> UndoItemMeta + Send + Sync>;
