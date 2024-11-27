use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::HashMap,
    ops::{ControlFlow, Deref},
    sync::{Arc, Mutex},
};

use loro::{
    cursor::CannotFindRelativePosition, ChangeTravelError, CounterSpan, DocAnalysis,
    FrontiersNotIncluded, IdSpan, JsonPathError, JsonSchema, Lamport, LoroDoc as InnerLoroDoc,
    LoroEncodeError, LoroError, LoroResult, PeerID, Timestamp, VersionRange, ID,
};

use crate::{
    event::{DiffEvent, Subscriber},
    AbsolutePosition, Configure, ContainerID, ContainerIdLike, Cursor, Frontiers, Index,
    LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText, LoroTree, LoroValue, StyleConfigMap,
    ValueOrContainer, VersionVector,
};

/// Decodes the metadata for an imported blob from the provided bytes.
#[inline]
pub fn decode_import_blob_meta(
    bytes: &[u8],
    check_checksum: bool,
) -> LoroResult<ImportBlobMetadata> {
    let s = InnerLoroDoc::decode_import_blob_meta(bytes, check_checksum)?;
    Ok(s.into())
}

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

    pub fn fork_at(&self, frontiers: &Frontiers) -> Arc<Self> {
        let doc = self.doc.fork_at(&frontiers.into());
        Arc::new(LoroDoc { doc })
    }

    /// Get the configurations of the document.
    #[inline]
    pub fn config(&self) -> Arc<Configure> {
        Arc::new(self.doc.config().clone().into())
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
    #[inline]
    pub fn get_change(&self, id: ID) -> Option<ChangeMeta> {
        self.doc.get_change(id).map(|x| x.into())
    }

    /// Set whether to record the timestamp of each change. Default is `false`.
    ///
    /// If enabled, the Unix timestamp will be recorded for each change automatically.
    ///
    /// You can set each timestamp manually when committing a change.
    ///
    /// NOTE: Timestamps are forced to be in ascending order.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    #[inline]
    pub fn set_record_timestamp(&self, record: bool) {
        self.doc.set_record_timestamp(record);
    }

    /// Set the interval of mergeable changes, in milliseconds.
    ///
    /// If two continuous local changes are within the interval, they will be merged into one change.
    /// The default value is 1000 seconds.
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
    #[inline]
    pub fn config_text_style(&self, text_style: Arc<StyleConfigMap>) {
        self.doc.config_text_style(text_style.as_ref().to_loro())
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
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// You should call `attach` to attach the `DocState` to the latest version of `OpLog`.
    #[inline]
    pub fn checkout(&self, frontiers: &Frontiers) -> LoroResult<()> {
        self.doc.checkout(&frontiers.into())
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
        self.doc.cmp_with_frontiers(&other.into())
    }

    // TODO:
    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        self.doc.cmp_frontiers(&a.into(), &b.into())
    }

    /// Force the document enter the detached mode.
    ///
    /// In this mode, when you importing new updates, the [loro_internal::DocState] will not be changed.
    ///
    /// Learn more at https://loro.dev/docs/advanced/doc_state_and_oplog#attacheddetached-status
    #[inline]
    pub fn detach(&self) {
        self.doc.detach()
    }

    /// Import a batch of updates/snapshot.
    ///
    /// The data can be in arbitrary order. The import result will be the same.
    #[inline]
    pub fn import_batch(&self, bytes: &[Vec<u8>]) -> LoroResult<()> {
        self.doc.import_batch(bytes)
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

    /// Commit the cumulative auto commit transaction.
    ///
    /// There is a transaction behind every operation.
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    #[inline]
    pub fn commit(&self) {
        self.doc.commit()
    }

    pub fn commit_with(&self, options: CommitOptions) {
        self.doc.commit_with(options.into())
    }

    /// Set commit message for the current uncommitted changes
    pub fn set_next_commit_message(&self, msg: &str) {
        self.doc.set_next_commit_message(msg)
    }

    /// Whether the document is in detached mode, where the [loro_internal::DocState] is not
    /// synchronized with the latest version of the [loro_internal::OpLog].
    #[inline]
    pub fn is_detached(&self) -> bool {
        self.doc.is_detached()
    }

    /// Import updates/snapshot exported by [`LoroDoc::export_snapshot`] or [`LoroDoc::export_from`].
    #[inline]
    pub fn import(&self, bytes: &[u8]) -> Result<ImportStatus, LoroError> {
        let status = self.doc.import_with(bytes, "")?;
        Ok(status.into())
    }

    /// Import updates/snapshot exported by [`LoroDoc::export_snapshot`] or [`LoroDoc::export_from`].
    ///
    /// It marks the import with a custom `origin` string. It can be used to track the import source
    /// in the generated events.
    #[inline]
    pub fn import_with(&self, bytes: &[u8], origin: &str) -> Result<ImportStatus, LoroError> {
        let status = self.doc.import_with(bytes, origin)?;
        Ok(status.into())
    }

    pub fn import_json_updates(&self, json: &str) -> Result<ImportStatus, LoroError> {
        let status = self.doc.import_json_updates(json)?;
        Ok(status.into())
    }

    /// Export the current state with json-string format of the document.
    #[inline]
    pub fn export_json_updates(&self, start_vv: &VersionVector, end_vv: &VersionVector) -> String {
        let json = self
            .doc
            .export_json_updates(&start_vv.into(), &end_vv.into());
        serde_json::to_string(&json).unwrap()
    }

    // TODO: add export method
    /// Export all the ops not included in the given `VersionVector`
    #[inline]
    #[allow(deprecated)]
    #[deprecated(
        since = "1.0.0",
        note = "Use `export` with `ExportMode::Updates` instead"
    )]
    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        self.doc.export_from(&vv.into())
    }

    /// Export the current state and history of the document.
    #[inline]
    pub fn export_snapshot(&self) -> Vec<u8> {
        #[allow(deprecated)]
        self.doc.export_snapshot()
    }

    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<Arc<VersionVector>> {
        self.doc
            .frontiers_to_vv(&frontiers.into())
            .map(|v| Arc::new(v.into()))
    }

    pub fn minimize_frontiers(&self, frontiers: &Frontiers) -> FrontiersOrID {
        match self.doc.minimize_frontiers(&frontiers.into()) {
            Ok(f) => FrontiersOrID {
                frontiers: Some(Arc::new(f.into())),
                id: None,
            },
            Err(id) => FrontiersOrID {
                frontiers: None,
                id: Some(id),
            },
        }
    }

    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Arc<Frontiers> {
        Arc::new(self.doc.vv_to_frontiers(&vv.into()).into())
    }

    // TODO: with oplog
    // TODO: with state

    pub fn oplog_vv(&self) -> Arc<VersionVector> {
        Arc::new(self.doc.oplog_vv().into())
    }

    pub fn state_vv(&self) -> Arc<VersionVector> {
        Arc::new(self.doc.state_vv().into())
    }

    /// Get the `VersionVector` of the start of the shallow history
    ///
    /// The ops included by the shallow history are not in the doc.
    #[inline]
    pub fn shallow_since_vv(&self) -> Arc<VersionVector> {
        Arc::new(loro::VersionVector::from_im_vv(&self.doc.shallow_since_vv()).into())
    }

    /// Get the total number of operations in the `OpLog`
    #[inline]
    pub fn len_ops(&self) -> u64 {
        self.doc.len_ops() as u64
    }

    /// Get the total number of changes in the `OpLog`
    #[inline]
    pub fn len_changes(&self) -> u64 {
        self.doc.len_changes() as u64
    }

    /// Get the shallow value of the document.
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.doc.get_value().into()
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.doc.get_deep_value().into()
    }

    /// Get the current state with container id of the doc
    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.doc.get_deep_value_with_id().into()
    }

    pub fn oplog_frontiers(&self) -> Arc<Frontiers> {
        Arc::new(self.doc.oplog_frontiers().into())
    }

    pub fn state_frontiers(&self) -> Arc<Frontiers> {
        Arc::new(self.doc.state_frontiers().into())
    }

    /// Get the PeerID
    #[inline]
    pub fn peer_id(&self) -> PeerID {
        self.doc.peer_id()
    }

    /// Change the PeerID
    ///
    /// NOTE: You need ot make sure there is no chance two peer have the same PeerID.
    /// If it happens, the document will be corrupted.
    #[inline]
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        self.doc.set_peer_id(peer)
    }

    pub fn subscribe(
        &self,
        container_id: &ContainerID,
        subscriber: Arc<dyn Subscriber>,
    ) -> Arc<Subscription> {
        Arc::new(
            self.doc
                .subscribe(
                    &(container_id.into()),
                    Arc::new(move |e| {
                        subscriber.on_diff(DiffEvent::from(e));
                    }),
                )
                .into(),
        )
    }

    pub fn subscribe_root(&self, subscriber: Arc<dyn Subscriber>) -> Arc<Subscription> {
        // self.doc.subscribe_root(callback)
        Arc::new(
            self.doc
                .subscribe_root(Arc::new(move |e| {
                    subscriber.on_diff(DiffEvent::from(e));
                }))
                .into(),
        )
    }

    /// Subscribe the local update of the document.
    pub fn subscribe_local_update(
        &self,
        callback: Arc<dyn LocalUpdateCallback>,
    ) -> Arc<Subscription> {
        let s = self.doc.subscribe_local_update(Box::new(move |update| {
            // TODO: should it be cloned?
            callback.on_local_update(update.to_vec());
            true
        }));
        Arc::new(Subscription(Mutex::new(Some(s))))
    }

    /// Estimate the size of the document states in memory.
    #[inline]
    pub fn log_estimate_size(&self) {
        self.doc.log_estimate_size();
    }

    /// Check the correctness of the document state by comparing it with the state
    /// calculated by applying all the history.
    #[inline]
    pub fn check_state_correctness_slow(&self) {
        self.doc.check_state_correctness_slow()
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
        let loro::cursor::PosQueryResult { update, current } = self.doc.get_cursor_pos(cursor)?;
        Ok(PosQueryResult {
            current: AbsolutePosition {
                pos: current.pos as u32,
                side: current.side,
            },
            update: update.map(|x| Arc::new(x.into())),
        })
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
    /// The parsed ops will be dropped
    #[inline]
    pub fn compact_change_store(&self) {
        self.doc.compact_change_store()
    }

    // TODO: https://github.com/mozilla/uniffi-rs/issues/1372
    /// Export the document in the given mode.
    // pub fn export(&self, mode: ExportMode) -> Vec<u8> {
    //     self.doc.export(mode.into())
    // }

    pub fn export_updates_in_range(&self, spans: &[IdSpan]) -> Vec<u8> {
        self.doc
            .export(loro::ExportMode::UpdatesInRange {
                spans: Cow::Borrowed(spans),
            })
            .unwrap()
    }

    pub fn export_shallow_snapshot(&self, frontiers: &Frontiers) -> Vec<u8> {
        self.doc
            .export(loro::ExportMode::ShallowSnapshot(Cow::Owned(
                frontiers.into(),
            )))
            .unwrap()
    }

    pub fn export_state_only(
        &self,
        frontiers: Option<Arc<Frontiers>>,
    ) -> Result<Vec<u8>, LoroEncodeError> {
        self.doc
            .export(loro::ExportMode::StateOnly(frontiers.map(|x| {
                let a = x.as_ref();
                Cow::Owned(loro::Frontiers::from(a))
            })))
    }

    // TODO: impl
    /// Analyze the container info of the doc
    ///
    /// This is used for development and debugging. It can be slow.
    pub fn analyze(&self) -> DocAnalysis {
        self.doc.analyze()
    }

    /// Get the path from the root to the container
    pub fn get_path_to_container(&self, id: &ContainerID) -> Option<Vec<ContainerPath>> {
        self.doc.get_path_to_container(&id.into()).map(|x| {
            x.into_iter()
                .map(|(id, idx)| ContainerPath {
                    id: id.into(),
                    path: (&idx).into(),
                })
                .collect()
        })
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
    #[inline]
    pub fn jsonpath(&self, path: &str) -> Result<Vec<Arc<dyn ValueOrContainer>>, JsonPathError> {
        self.doc.jsonpath(path).map(|vec| {
            vec.into_iter()
                .map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>)
                .collect()
        })
    }

    pub fn travel_change_ancestors(
        &self,
        ids: &[ID],
        f: Arc<dyn ChangeAncestorsTraveler>,
    ) -> Result<(), ChangeTravelError> {
        self.doc
            .travel_change_ancestors(ids, &mut |change| match f.travel(change.into()) {
                true => ControlFlow::Continue(()),
                false => ControlFlow::Break(()),
            })
    }

    pub fn get_changed_containers_in(&self, id: ID, len: u32) -> Vec<ContainerID> {
        self.doc
            .get_changed_containers_in(id, len as usize)
            .into_iter()
            .map(|x| x.into())
            .collect()
    }

    pub fn is_shallow(&self) -> bool {
        self.doc.is_shallow()
    }

    pub fn get_pending_txn_len(&self) -> u32 {
        self.doc.get_pending_txn_len() as u32
    }
}

pub trait ChangeAncestorsTraveler: Sync + Send {
    fn travel(&self, change: ChangeMeta) -> bool;
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

pub struct ChangeMeta {
    /// Lamport timestamp of the Change
    pub lamport: Lamport,
    /// The first Op id of the Change
    pub id: ID,
    /// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
    /// It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
    pub timestamp: Timestamp,
    /// The commit message of the change
    pub message: Option<String>,
    /// The dependencies of the first op of the change
    pub deps: Arc<Frontiers>,
    /// The total op num inside this change
    pub len: u32,
}

impl From<loro::ChangeMeta> for ChangeMeta {
    fn from(value: loro::ChangeMeta) -> Self {
        Self {
            lamport: value.lamport,
            id: value.id,
            timestamp: value.timestamp,
            message: value.message.map(|x| (*x).to_string()),
            deps: Arc::new(value.deps.into()),
            len: value.len as u32,
        }
    }
}

pub struct ImportBlobMetadata {
    /// The partial start version vector.
    ///
    /// Import blob includes all the ops from `partial_start_vv` to `partial_end_vv`.
    /// However, it does not constitute a complete version vector, as it only contains counters
    /// from peers included within the import blob.
    pub partial_start_vv: Arc<VersionVector>,
    /// The partial end version vector.
    ///
    /// Import blob includes all the ops from `partial_start_vv` to `partial_end_vv`.
    /// However, it does not constitute a complete version vector, as it only contains counters
    /// from peers included within the import blob.
    pub partial_end_vv: Arc<VersionVector>,
    pub start_timestamp: i64,
    pub start_frontiers: Arc<Frontiers>,
    pub end_timestamp: i64,
    pub change_num: u32,
    pub mode: String,
}

impl From<loro::ImportBlobMetadata> for ImportBlobMetadata {
    fn from(value: loro::ImportBlobMetadata) -> Self {
        Self {
            partial_start_vv: Arc::new(value.partial_start_vv.into()),
            partial_end_vv: Arc::new(value.partial_end_vv.into()),
            start_timestamp: value.start_timestamp,
            start_frontiers: Arc::new(value.start_frontiers.into()),
            end_timestamp: value.end_timestamp,
            change_num: value.change_num,
            mode: value.mode.to_string(),
        }
    }
}

pub struct CommitOptions {
    pub origin: Option<String>,
    pub immediate_renew: bool,
    pub timestamp: Option<Timestamp>,
    pub commit_msg: Option<String>,
}

impl From<CommitOptions> for loro::CommitOptions {
    fn from(value: CommitOptions) -> Self {
        loro::CommitOptions {
            origin: value.origin.map(|x| x.into()),
            immediate_renew: value.immediate_renew,
            timestamp: value.timestamp,
            commit_msg: value.commit_msg.map(|x| x.into()),
        }
    }
}

pub trait JsonSchemaLike {
    fn to_json_schema(&self) -> LoroResult<JsonSchema>;
}

impl<T: TryInto<JsonSchema> + Clone> JsonSchemaLike for T {
    fn to_json_schema(&self) -> LoroResult<JsonSchema> {
        self.clone()
            .try_into()
            .map_err(|_| LoroError::InvalidJsonSchema)
    }
}

pub trait LocalUpdateCallback: Sync + Send {
    fn on_local_update(&self, update: Vec<u8>);
}

pub trait Unsubscriber: Sync + Send {
    fn on_unsubscribe(&self);
}

/// A handle to a subscription created by GPUI. When dropped, the subscription
/// is cancelled and the callback will no longer be invoked.
pub struct Subscription(Mutex<Option<loro::Subscription>>);

impl Subscription {
    pub fn detach(self: Arc<Self>) {
        let s = self.0.try_lock().unwrap().take().unwrap();
        s.detach();
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Subscription")
    }
}

impl From<loro::Subscription> for Subscription {
    fn from(value: loro::Subscription) -> Self {
        Self(Mutex::new(Some(value)))
    }
}

pub struct PosQueryResult {
    pub update: Option<Arc<Cursor>>,
    pub current: AbsolutePosition,
}

pub enum ExportMode {
    Snapshot,
    Updates { from: VersionVector },
    UpdatesInRange { spans: Vec<IdSpan> },
    ShallowSnapshot { frontiers: Frontiers },
    StateOnly { frontiers: Option<Frontiers> },
}

impl From<ExportMode> for loro::ExportMode<'_> {
    fn from(value: ExportMode) -> Self {
        match value {
            ExportMode::Snapshot => loro::ExportMode::Snapshot,
            ExportMode::Updates { from } => loro::ExportMode::Updates {
                from: Cow::Owned(from.into()),
            },
            ExportMode::UpdatesInRange { spans } => loro::ExportMode::UpdatesInRange {
                spans: Cow::Owned(spans),
            },
            ExportMode::ShallowSnapshot { frontiers } => {
                loro::ExportMode::ShallowSnapshot(Cow::Owned(frontiers.into()))
            }
            ExportMode::StateOnly { frontiers } => {
                loro::ExportMode::StateOnly(frontiers.map(|x| Cow::Owned(x.into())))
            }
        }
    }
}

pub struct ContainerPath {
    pub id: ContainerID,
    pub path: Index,
}

pub struct ImportStatus {
    pub success: HashMap<u64, CounterSpan>,
    pub pending: Option<HashMap<u64, CounterSpan>>,
}

impl From<loro::ImportStatus> for ImportStatus {
    fn from(value: loro::ImportStatus) -> Self {
        let a = &value.success;
        Self {
            success: vr_to_map(a),
            pending: value.pending.as_ref().map(vr_to_map),
        }
    }
}

fn vr_to_map(a: &VersionRange) -> HashMap<u64, CounterSpan> {
    a.iter()
        .map(|x| {
            (
                *x.0,
                CounterSpan {
                    start: x.1 .0,
                    end: x.1 .1,
                },
            )
        })
        .collect()
}

pub struct FrontiersOrID {
    pub frontiers: Option<Arc<Frontiers>>,
    pub id: Option<ID>,
}
