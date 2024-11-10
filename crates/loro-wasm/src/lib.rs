//! Loro WASM bindings.
#![allow(non_snake_case)]
#![allow(clippy::empty_docs)]
#![allow(clippy::doc_lazy_continuation)]
// #![warn(missing_docs)]

use convert::{import_status_to_js_value, js_to_version_vector, resolved_diff_to_js};
use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use loro_internal::{
    change::Lamport,
    configure::{StyleConfig, StyleConfigMap},
    container::{richtext::ExpandType, ContainerID},
    cursor::{self, Side},
    encoding::ImportBlobMetadata,
    event::Index,
    handler::{
        Handler, ListHandler, MapHandler, TextDelta, TextHandler, TreeHandler, UpdateOptions,
        ValueOrHandler,
    },
    id::{Counter, PeerID, TreeID, ID},
    json::JsonSchema,
    loro::{CommitOptions, ExportMode},
    loro_common::check_root_container_name,
    undo::{UndoItemMeta, UndoOrRedo},
    version::Frontiers,
    ContainerType, DiffEvent, FxHashMap, HandlerTrait, LoroDoc as LoroDocInner, LoroResult,
    LoroValue, MovableListHandler, Subscription, TreeNodeWithChildren, TreeParentId,
    UndoManager as InnerUndoManager, VersionVector as InternalVersionVector,
};
use rle::HasLength;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, cmp::Ordering, ops::ControlFlow, rc::Rc, sync::Arc};
use wasm_bindgen::{__rt::IntoJsResult, prelude::*, throw_val};
use wasm_bindgen_derive::TryFromJsValue;

mod counter;
pub use counter::LoroCounter;

mod awareness;
mod log;

use crate::convert::{handler_to_js_value, js_to_container, js_to_cursor};
pub use awareness::AwarenessWasm;

mod convert;

#[wasm_bindgen(start)]
fn run() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn encodeFrontiers(frontiers: Vec<JsID>) -> JsResult<Vec<u8>> {
    let frontiers = ids_to_frontiers(frontiers)?;
    let encoded = frontiers.encode();
    Ok(encoded)
}

#[wasm_bindgen]
pub fn decodeFrontiers(bytes: &[u8]) -> JsResult<JsIDs> {
    let frontiers =
        Frontiers::decode(bytes).map_err(|_| JsError::new("Invalid frontiers binary data"))?;
    Ok(frontiers_to_ids(&frontiers))
}

/// Enable debug info of Loro
#[wasm_bindgen(js_name = setDebug)]
pub fn set_debug() {
    tracing_wasm::set_as_global_default();
}

type JsResult<T> = Result<T, JsValue>;

/// The CRDTs document. Loro supports different CRDTs include [**List**](LoroList),
/// [**RichText**](LoroText), [**Map**](LoroMap) and [**Movable Tree**](LoroTree),
/// you could build all kind of applications by these.
///
/// @example
/// ```ts
/// import { LoroDoc } from "loro-crdt"
///
/// const loro = new LoroDoc();
/// const text = loro.getText("text");
/// const list = loro.getList("list");
/// const map = loro.getMap("Map");
/// const tree = loro.getTree("tree");
/// ```
#[wasm_bindgen]
pub struct LoroDoc(Arc<LoroDocInner>);

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "number | bigint | `${number}`")]
    pub type JsIntoPeerID;
    #[wasm_bindgen(typescript_type = "PeerID")]
    pub type JsStrPeerID;
    #[wasm_bindgen(typescript_type = "ContainerID")]
    pub type JsContainerID;
    #[wasm_bindgen(typescript_type = "ContainerID | string")]
    pub type JsIntoContainerID;
    #[wasm_bindgen(typescript_type = "Transaction | LoroDoc")]
    pub type JsTransaction;
    #[wasm_bindgen(typescript_type = "string | undefined")]
    pub type JsOrigin;
    #[wasm_bindgen(typescript_type = "{ peer: PeerID, counter: number }")]
    pub type JsID;
    #[wasm_bindgen(typescript_type = "{ peer: PeerID, counter: number }[]")]
    pub type JsIDs;
    #[wasm_bindgen(typescript_type = "{ start: number, end: number }")]
    pub type JsRange;
    #[wasm_bindgen(typescript_type = "number|bool|string|null")]
    pub type JsMarkValue;
    #[wasm_bindgen(typescript_type = "TreeID")]
    pub type JsTreeID;
    #[wasm_bindgen(typescript_type = "TreeID | undefined")]
    pub type JsParentTreeID;
    #[wasm_bindgen(typescript_type = "{ withDeleted: boolean }")]
    pub type JsGetNodesProp;
    #[wasm_bindgen(typescript_type = "LoroTreeNode | undefined")]
    pub type JsTreeNodeOrUndefined;
    #[wasm_bindgen(typescript_type = "string | undefined")]
    pub type JsPositionOrUndefined;
    #[wasm_bindgen(typescript_type = "Delta<string>[]")]
    pub type JsStringDelta;
    #[wasm_bindgen(typescript_type = "Map<PeerID, number>")]
    pub type JsVersionVectorMap;
    #[wasm_bindgen(typescript_type = "Map<PeerID, Change[]>")]
    pub type JsChanges;
    #[wasm_bindgen(typescript_type = "Change")]
    pub type JsChange;
    #[wasm_bindgen(typescript_type = "Change | undefined")]
    pub type JsChangeOrUndefined;
    #[wasm_bindgen(
        typescript_type = "Map<PeerID, number> | Uint8Array | VersionVector | undefined | null"
    )]
    pub type JsIntoVersionVector;
    #[wasm_bindgen(typescript_type = "Container")]
    pub type JsContainer;
    #[wasm_bindgen(typescript_type = "Value")]
    pub type JsLoroValue;
    #[wasm_bindgen(typescript_type = "Value | Container")]
    pub type JsValueOrContainer;
    #[wasm_bindgen(typescript_type = "Value | Container | undefined")]
    pub type JsValueOrContainerOrUndefined;
    #[wasm_bindgen(typescript_type = "Container | undefined")]
    pub type JsContainerOrUndefined;
    #[wasm_bindgen(typescript_type = "LoroText | undefined")]
    pub type JsLoroTextOrUndefined;
    #[wasm_bindgen(typescript_type = "LoroMap | undefined")]
    pub type JsLoroMapOrUndefined;
    #[wasm_bindgen(typescript_type = "LoroList | undefined")]
    pub type JsLoroListOrUndefined;
    #[wasm_bindgen(typescript_type = "LoroTree | undefined")]
    pub type JsLoroTreeOrUndefined;
    #[wasm_bindgen(typescript_type = "[string, Value | Container]")]
    pub type MapEntry;
    #[wasm_bindgen(typescript_type = "{[key: string]: { expand: 'before'|'after'|'none'|'both' }}")]
    pub type JsTextStyles;
    #[wasm_bindgen(typescript_type = "Delta<string>[]")]
    pub type JsDelta;
    #[wasm_bindgen(typescript_type = "-1 | 1 | 0 | undefined")]
    pub type JsPartialOrd;
    #[wasm_bindgen(typescript_type = "'Tree'|'Map'|'List'|'Text'")]
    pub type JsContainerKind;
    #[wasm_bindgen(typescript_type = "'Text'")]
    pub type JsTextStr;
    #[wasm_bindgen(typescript_type = "'Tree'")]
    pub type JsTreeStr;
    #[wasm_bindgen(typescript_type = "'Map'")]
    pub type JsMapStr;
    #[wasm_bindgen(typescript_type = "'List'")]
    pub type JsListStr;
    #[wasm_bindgen(typescript_type = "'MovableList'")]
    pub type JsMovableListStr;
    #[wasm_bindgen(typescript_type = "ImportBlobMetadata")]
    pub type JsImportBlobMetadata;
    #[wasm_bindgen(typescript_type = "Side")]
    pub type JsSide;
    #[wasm_bindgen(typescript_type = "{ update?: Cursor, offset: number, side: Side }")]
    pub type JsCursorQueryAns;
    #[wasm_bindgen(typescript_type = "UndoConfig")]
    pub type JsUndoConfig;
    #[wasm_bindgen(typescript_type = "JsonSchema")]
    pub type JsJsonSchema;
    #[wasm_bindgen(typescript_type = "string | JsonSchema")]
    pub type JsJsonSchemaOrString;
    #[wasm_bindgen(typescript_type = "ExportMode")]
    pub type JsExportMode;
    #[wasm_bindgen(typescript_type = "{ origin?: string, timestamp?: number, message?: string }")]
    pub type JsCommitOption;
    #[wasm_bindgen(typescript_type = "ImportStatus")]
    pub type JsImportStatus;
    #[wasm_bindgen(typescript_type = "(change: ChangeMeta) => boolean")]
    pub type JsTravelChangeFunction;
}

mod observer {
    use std::thread::ThreadId;

    use wasm_bindgen::JsValue;

    use crate::JsResult;

    /// We need to wrap the observer function in a struct so that we can implement Send for it.
    /// But it's not Send essentially, so we need to check it manually in runtime.
    #[derive(Clone)]
    pub(crate) struct Observer {
        f: js_sys::Function,
        thread: ThreadId,
    }

    impl Observer {
        pub fn new(f: js_sys::Function) -> Self {
            Self {
                f,
                thread: std::thread::current().id(),
            }
        }

        pub fn call1(&self, arg: &JsValue) -> JsResult<JsValue> {
            if std::thread::current().id() == self.thread {
                self.f.call1(&JsValue::NULL, arg)
            } else {
                Err(JsValue::from_str("Observer called from different thread"))
            }
        }

        pub fn call2(&self, arg1: &JsValue, arg2: &JsValue) -> JsResult<JsValue> {
            if std::thread::current().id() == self.thread {
                self.f.call2(&JsValue::NULL, arg1, arg2)
            } else {
                Err(JsValue::from_str("Observer called from different thread"))
            }
        }

        pub fn call3(&self, arg1: &JsValue, arg2: &JsValue, arg3: &JsValue) -> JsResult<JsValue> {
            if std::thread::current().id() == self.thread {
                self.f.call3(&JsValue::NULL, arg1, arg2, arg3)
            } else {
                Err(JsValue::from_str("Observer called from different thread"))
            }
        }
    }

    unsafe impl Send for Observer {}
    unsafe impl Sync for Observer {}
}

fn ids_to_frontiers(ids: Vec<JsID>) -> JsResult<Frontiers> {
    let mut frontiers = Frontiers::default();
    for id in ids {
        let id = js_id_to_id(id)?;
        frontiers.push(id);
    }

    Ok(frontiers)
}

fn id_to_js(id: &ID) -> JsValue {
    let obj = Object::new();
    Reflect::set(&obj, &"peer".into(), &id.peer.to_string().into()).unwrap();
    Reflect::set(&obj, &"counter".into(), &id.counter.into()).unwrap();
    let value: JsValue = obj.into_js_result().unwrap();
    value
}

fn peer_id_to_js(peer: PeerID) -> JsStrPeerID {
    let v: JsValue = peer.to_string().into();
    v.into()
}

fn js_id_to_id(id: JsID) -> Result<ID, JsValue> {
    let peer = js_peer_to_peer(Reflect::get(&id, &"peer".into())?)?;
    let counter = Reflect::get(&id, &"counter".into())?.as_f64().unwrap() as Counter;
    let id = ID::new(peer, counter);
    Ok(id)
}

fn frontiers_to_ids(frontiers: &Frontiers) -> JsIDs {
    let js_arr = Array::new();
    for id in frontiers.iter() {
        let value = id_to_js(&id);
        js_arr.push(&value);
    }

    let value: JsValue = js_arr.into();
    value.into()
}

fn js_value_to_container_id(
    cid: &JsIntoContainerID,
    kind: ContainerType,
) -> Result<ContainerID, JsValue> {
    if !cid.is_string() {
        return Err(JsValue::from_str(&format!(
            "ContainerID must be a string, but found {}",
            cid.js_typeof().as_string().unwrap(),
        )));
    }

    let s = cid.as_string().unwrap();
    if let Ok(cid) = ContainerID::try_from(s.as_str()) {
        Ok(cid)
    } else if check_root_container_name(s.as_str()) {
        Ok(ContainerID::new_root(s.as_str(), kind))
    } else {
        Err(JsValue::from_str(
            "Invalid root container name! Don't include '/' or '\\0'",
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
struct StringID {
    peer: String,
    counter: Counter,
}

#[derive(Debug, Clone, Serialize)]
struct ChangeMeta {
    lamport: Lamport,
    length: u32,
    peer: String,
    counter: Counter,
    deps: Vec<StringID>,
    timestamp: f64,
    message: Option<Arc<str>>,
}

impl ChangeMeta {
    fn to_js(&self) -> JsValue {
        let s = serde_wasm_bindgen::Serializer::new();
        self.serialize(&s).unwrap()
    }
}

#[wasm_bindgen]
impl LoroDoc {
    /// Create a new loro document.
    ///
    /// New document will have a random peer id.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let doc = LoroDocInner::new();
        doc.start_auto_commit();
        Self(Arc::new(doc))
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
    #[wasm_bindgen(js_name = "setDetachedEditing")]
    pub fn set_detached_editing(&self, enable: bool) {
        self.0.set_detached_editing(enable);
    }

    /// Whether the editing is enabled in detached mode.
    ///
    /// The doc enter detached mode after calling `detach` or checking out a non-latest version.
    ///
    /// # Important Notes:
    ///
    /// - This mode uses a different PeerID for each checkout.
    /// - Ensure no concurrent operations share the same PeerID if set manually.
    /// - Importing does not affect the document's state or version; changes are
    ///   recorded in the [OpLog] only. Call `checkout` to apply changes.
    #[wasm_bindgen(js_name = "isDetachedEditingEnabled")]
    pub fn is_detached_editing_enabled(&self) -> bool {
        self.0.is_detached_editing_enabled()
    }

    /// Set whether to record the timestamp of each change. Default is `false`.
    ///
    /// If enabled, the Unix timestamp (in seconds) will be recorded for each change automatically.
    ///
    /// You can also set each timestamp manually when you commit a change.
    /// The timestamp manually set will override the automatic one.
    ///
    /// NOTE: Timestamps are forced to be in ascending order in the OpLog's history.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    #[wasm_bindgen(js_name = "setRecordTimestamp")]
    pub fn set_record_timestamp(&self, auto_record: bool) {
        self.0.set_record_timestamp(auto_record);
    }

    /// If two continuous local changes are within the interval, they will be merged into one change.
    ///
    /// The default value is 1_000_000, the default unit is seconds.
    #[wasm_bindgen(js_name = "setChangeMergeInterval")]
    pub fn set_change_merge_interval(&self, interval: f64) {
        self.0.set_change_merge_interval(interval as i64);
    }

    /// Set the rich text format configuration of the document.
    ///
    /// You need to config it if you use rich text `mark` method.
    /// Specifically, you need to config the `expand` property of each style.
    ///
    /// Expand is used to specify the behavior of expanding when new text is inserted at the
    /// beginning or end of the style.
    ///
    /// You can specify the `expand` option to set the behavior when inserting text at the boundary of the range.
    ///
    /// - `after`(default): when inserting text right after the given range, the mark will be expanded to include the inserted text
    /// - `before`: when inserting text right before the given range, the mark will be expanded to include the inserted text
    /// - `none`: the mark will not be expanded to include the inserted text at the boundaries
    /// - `both`: when inserting text either right before or right after the given range, the mark will be expanded to include the inserted text
    ///
    /// @example
    /// ```ts
    /// const doc = new LoroDoc();
    /// doc.configTextStyle({
    ///   bold: { expand: "after" },
    ///   link: { expand: "before" }
    /// });
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// expect(text.toDelta()).toStrictEqual([
    ///   {
    ///     insert: "Hello",
    ///     attributes: {
    ///       bold: true,
    ///     },
    ///   },
    ///   {
    ///     insert: " World!",
    ///   },
    /// ] as Delta<string>[]);
    /// ```
    #[wasm_bindgen(js_name = "configTextStyle")]
    pub fn config_text_style(&self, styles: JsTextStyles) -> JsResult<()> {
        let mut style_config = StyleConfigMap::new();
        // read key value pair in styles
        for key in Reflect::own_keys(&styles)?.iter() {
            let value = Reflect::get(&styles, &key).unwrap();
            let key = key.as_string().unwrap();
            // Assert value is an object, otherwise throw an error with desc
            if !value.is_object() {
                return Err("Text style config format error".into());
            }
            // read expand value from value
            let expand = Reflect::get(&value, &"expand".into()).expect("`expand` not specified");
            let expand_str = expand.as_string().unwrap();
            // read allowOverlap value from value
            style_config.insert(
                key.into(),
                StyleConfig {
                    expand: ExpandType::try_from_str(&expand_str)
                        .expect("`expand` must be one of `none`, `start`, `end`, `both`"),
                },
            );
        }

        self.0.config_text_style(style_config);
        Ok(())
    }

    /// Create a loro document from the snapshot.
    ///
    /// @see You can learn more [here](https://loro.dev/docs/tutorial/encoding).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt"
    ///
    /// const doc = new LoroDoc();
    /// // ...
    /// const bytes = doc.export({ mode: "snapshot" });
    /// const loro = LoroDoc.fromSnapshot(bytes);
    /// ```
    ///
    #[wasm_bindgen(js_name = "fromSnapshot")]
    pub fn from_snapshot(snapshot: &[u8]) -> JsResult<LoroDoc> {
        let doc = LoroDocInner::from_snapshot(snapshot)?;
        doc.start_auto_commit();
        Ok(Self(Arc::new(doc)))
    }

    /// Attach the document state to the latest known version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// This method has the same effect as invoking `checkoutToLatest`.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// doc.checkout(frontiers);
    /// // you need call `attach()` or `checkoutToLatest()` before changing the doc.
    /// doc.attach();
    /// text.insert(0, "Hi");
    /// ```
    pub fn attach(&mut self) {
        self.0.attach();
    }

    /// `detached` indicates that the `DocState` is not synchronized with the latest version of `OpLog`.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable by default, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// console.log(doc.isDetached());  // false
    /// doc.checkout(frontiers);
    /// console.log(doc.isDetached());  // true
    /// doc.attach();
    /// console.log(doc.isDetached());  // false
    /// ```
    ///
    #[wasm_bindgen(js_name = "isDetached")]
    pub fn is_detached(&self) -> bool {
        self.0.is_detached()
    }

    /// Detach the document state from the latest known version.
    ///
    /// After detaching, all import operations will be recorded in the `OpLog` without being applied to the `DocState`.
    /// When `detached`, the document is not editable.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// doc.detach();
    /// console.log(doc.isDetached());  // true
    /// ```
    pub fn detach(&self) {
        self.0.detach()
    }

    /// Duplicate the document with a different PeerID
    ///
    /// The time complexity and space complexity of this operation are both O(n),
    ///
    /// When called in detached mode, it will fork at the current state frontiers.
    /// It will have the same effect as `forkAt(&self.frontiers())`.
    pub fn fork(&self) -> Self {
        Self(Arc::new(self.0.fork()))
    }

    /// Creates a new LoroDoc at a specified version (Frontiers)
    ///
    /// The created doc will only contain the history before the specified frontiers.
    #[wasm_bindgen(js_name = "forkAt")]
    pub fn fork_at(&self, frontiers: Vec<JsID>) -> JsResult<LoroDoc> {
        Ok(Self(Arc::new(
            self.0.fork_at(&ids_to_frontiers(frontiers)?),
        )))
    }

    /// Checkout the `DocState` to the latest version of `OpLog`.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable by default, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// This has the same effect as `attach`.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// doc.checkout(frontiers);
    /// // you need call `checkoutToLatest()` or `attach()` before changing the doc.
    /// doc.checkoutToLatest();
    /// text.insert(0, "Hi");
    /// ```
    #[wasm_bindgen(js_name = "checkoutToLatest")]
    pub fn checkout_to_latest(&mut self) -> JsResult<()> {
        self.0.checkout_to_latest();
        Ok(())
    }

    /// Visit all the ancestors of the changes in causal order.
    ///
    /// @param ids - the changes to visit
    /// @param f - the callback function, return `true` to continue visiting, return `false` to stop
    #[wasm_bindgen(js_name = "travelChangeAncestors")]
    pub fn travel_change_ancestors(
        &self,
        ids: Vec<JsID>,
        f: JsTravelChangeFunction,
    ) -> JsResult<()> {
        let f: js_sys::Function = match f.dyn_into::<js_sys::Function>() {
            Ok(f) => f,
            Err(_) => return Err(JsValue::from_str("Expected a function")),
        };
        let observer = observer::Observer::new(f);
        self.0
            .travel_change_ancestors(
                &ids.into_iter()
                    .map(|id| js_id_to_id(id).unwrap())
                    .collect::<Vec<_>>(),
                &mut |meta| {
                    let res = observer
                        .call1(
                            &ChangeMeta {
                                lamport: meta.lamport,
                                length: meta.len as u32,
                                peer: meta.id.peer.to_string(),
                                counter: meta.id.counter,
                                deps: meta
                                    .deps
                                    .iter()
                                    .map(|id| StringID {
                                        peer: id.peer.to_string(),
                                        counter: id.counter,
                                    })
                                    .collect(),
                                timestamp: meta.timestamp as f64,
                                message: meta.message,
                            }
                            .to_js(),
                        )
                        .unwrap();
                    if res.as_bool().unwrap() {
                        ControlFlow::Continue(())
                    } else {
                        ControlFlow::Break(())
                    }
                },
            )
            .map_err(|e| JsValue::from(e.to_string()))
    }

    /// Checkout the `DocState` to a specific version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// You should call `attach` to attach the `DocState` to the latest version of `OpLog`.
    ///
    /// @param frontiers - the specific frontiers
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// doc.checkout(frontiers);
    /// console.log(doc.toJSON()); // {"text": ""}
    /// ```
    pub fn checkout(&mut self, frontiers: Vec<JsID>) -> JsResult<()> {
        self.0.checkout(&ids_to_frontiers(frontiers)?)?;
        Ok(())
    }

    /// Peer ID of the current writer.
    #[wasm_bindgen(js_name = "peerId", method, getter)]
    pub fn peer_id(&self) -> u64 {
        self.0.peer_id()
    }

    /// Get peer id in decimal string.
    #[wasm_bindgen(js_name = "peerIdStr", method, getter)]
    pub fn peer_id_str(&self) -> JsStrPeerID {
        let v: JsValue = format!("{}", self.0.peer_id()).into();
        v.into()
    }

    /// Set the peer ID of the current writer.
    ///
    /// It must be a number, a BigInt, or a decimal string that can be parsed to a unsigned 64-bit integer.
    ///
    /// Note: use it with caution. You need to make sure there is not chance that two peers
    /// have the same peer ID. Otherwise, we cannot ensure the consistency of the document.
    #[wasm_bindgen(js_name = "setPeerId", method)]
    pub fn set_peer_id(&self, peer_id: JsIntoPeerID) -> JsResult<()> {
        let id = js_peer_to_peer(peer_id.into())?;
        self.0.set_peer_id(id)?;
        Ok(())
    }

    /// Commit the cumulative auto committed transaction.
    ///
    /// You can specify the `origin`, `timestamp`, and `message` of the commit.
    ///
    /// - The `origin` is used to mark the event
    /// - The `message` works like a git commit message, which will be recorded and synced to peers
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// NOTE: Timestamps are forced to be in ascending order.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    ///
    /// NOTE: The `origin` will not be persisted, but the `message` will.
    pub fn commit(&self, options: Option<JsCommitOption>) -> JsResult<()> {
        if let Some(options) = options {
            if !options.is_object() {
                return Err(JsValue::from_str("Commit options must be an object"));
            }
            let origin: Option<String> = Reflect::get(&options, &JsValue::from_str("origin"))
                .ok()
                .and_then(|x| x.as_string());
            let timestamp: Option<f64> = Reflect::get(&options, &JsValue::from_str("timestamp"))
                .ok()
                .and_then(|x| x.as_f64());
            let message: Option<String> = Reflect::get(&options, &JsValue::from_str("message"))
                .ok()
                .and_then(|x| x.as_string());

            let mut options = CommitOptions::default();
            options.set_origin(origin.as_deref());
            options.set_timestamp(timestamp.map(|x| x as i64));
            if let Some(msg) = message {
                options = options.commit_msg(&msg);
            }
            self.0.commit_with(options);
        } else {
            self.0.commit_with(CommitOptions::default());
        }
        Ok(())
    }

    /// Get the number of operations in the pending transaction.
    ///
    /// The pending transaction is the one that is not committed yet. It will be committed
    /// automatically after calling `doc.commit()`, `doc.export(mode)` or `doc.checkout(version)`.
    #[wasm_bindgen(js_name = "getPendingTxnLength")]
    pub fn get_pending_txn_len(&self) -> usize {
        self.0.get_pending_txn_len()
    }

    /// Get a LoroText by container id.
    ///
    /// The object returned is a new js object each time because it need to cross
    /// the WASM boundary.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// ```
    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&self, cid: &JsIntoContainerID) -> JsResult<LoroText> {
        let text = self
            .0
            .get_text(js_value_to_container_id(cid, ContainerType::Text)?);
        Ok(LoroText {
            handler: text,
            doc: Some(self.0.clone()),
            delta_cache: None,
        })
    }

    /// Get a LoroMap by container id
    ///
    /// The object returned is a new js object each time because it need to cross
    /// the WASM boundary.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// ```
    #[wasm_bindgen(js_name = "getMap", skip_typescript)]
    pub fn get_map(&self, cid: &JsIntoContainerID) -> JsResult<LoroMap> {
        let map = self
            .0
            .get_map(js_value_to_container_id(cid, ContainerType::Map)?);
        Ok(LoroMap {
            handler: map,
            doc: Some(self.0.clone()),
        })
    }

    /// Get a LoroList by container id
    ///
    /// The object returned is a new js object each time because it need to cross
    /// the WASM boundary.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// ```
    #[wasm_bindgen(js_name = "getList", skip_typescript)]
    pub fn get_list(&self, cid: &JsIntoContainerID) -> JsResult<LoroList> {
        let list = self
            .0
            .get_list(js_value_to_container_id(cid, ContainerType::List)?);
        Ok(LoroList {
            handler: list,
            doc: Some(self.0.clone()),
        })
    }

    /// Get a LoroMovableList by container id
    ///
    /// The object returned is a new js object each time because it need to cross
    /// the WASM boundary.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getMovableList("list");
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn getMovableList(&self, cid: &JsIntoContainerID) -> JsResult<LoroMovableList> {
        let list = self
            .0
            .get_movable_list(js_value_to_container_id(cid, ContainerType::MovableList)?);
        Ok(LoroMovableList {
            handler: list,
            doc: Some(self.0.clone()),
        })
    }

    /// Get a LoroCounter by container id
    #[wasm_bindgen(js_name = "getCounter")]
    pub fn get_counter(&self, cid: &JsIntoContainerID) -> JsResult<LoroCounter> {
        let counter = self
            .0
            .get_counter(js_value_to_container_id(cid, ContainerType::Counter)?);
        Ok(LoroCounter {
            handler: counter,
            doc: Some(self.0.clone()),
        })
    }

    /// Get a LoroTree by container id
    ///
    /// The object returned is a new js object each time because it need to cross
    /// the WASM boundary.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// ```
    #[wasm_bindgen(js_name = "getTree", skip_typescript)]
    pub fn get_tree(&self, cid: &JsIntoContainerID) -> JsResult<LoroTree> {
        let tree = self
            .0
            .get_tree(js_value_to_container_id(cid, ContainerType::Tree)?);
        Ok(LoroTree {
            handler: tree,
            doc: Some(self.0.clone()),
        })
    }

    /// Get the container corresponding to the container id
    ///
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// let text = doc.getText("text");
    /// const textId = text.id;
    /// text = doc.getContainerById(textId);
    /// ```
    #[wasm_bindgen(skip_typescript, js_name = "getContainerById")]
    pub fn get_container_by_id(&self, container_id: JsContainerID) -> JsResult<JsValue> {
        let container_id: ContainerID = container_id.to_owned().try_into()?;
        let ty = container_id.container_type();
        Ok(match ty {
            ContainerType::Map => {
                let map = self.0.get_map(container_id);
                LoroMap {
                    handler: map,
                    doc: Some(self.0.clone()),
                }
                .into()
            }
            ContainerType::List => {
                let list = self.0.get_list(container_id);
                LoroList {
                    handler: list,
                    doc: Some(self.0.clone()),
                }
                .into()
            }
            ContainerType::Text => {
                let richtext = self.0.get_text(container_id);
                LoroText {
                    handler: richtext,
                    doc: Some(self.0.clone()),
                    delta_cache: None,
                }
                .into()
            }
            ContainerType::Tree => {
                let tree = self.0.get_tree(container_id);
                LoroTree {
                    handler: tree,
                    doc: Some(self.0.clone()),
                }
                .into()
            }
            ContainerType::MovableList => {
                let movelist = self.0.get_movable_list(container_id);
                LoroMovableList {
                    handler: movelist,
                    doc: Some(self.0.clone()),
                }
                .into()
            }
            ContainerType::Counter => {
                let counter = self.0.get_counter(container_id);
                LoroCounter {
                    handler: counter,
                    doc: Some(self.0.clone()),
                }
                .into()
            }
            ContainerType::Unknown(_) => {
                return Err(JsValue::from_str(
                    "You are attempting to get an unknown container",
                ));
            }
        })
    }

    /// Set the commit message of the next commit
    #[wasm_bindgen(js_name = "setNextCommitMessage")]
    pub fn set_next_commit_message(&self, msg: &str) {
        self.0.set_next_commit_message(msg);
    }

    /// Get deep value of the document with container id
    #[wasm_bindgen(js_name = "getDeepValueWithID")]
    pub fn get_deep_value_with_id(&self) -> JsValue {
        self.0.get_deep_value_with_id().into()
    }

    /// Get the path from the root to the container
    #[wasm_bindgen(js_name = "getPathToContainer")]
    pub fn get_path_to_container(&self, id: JsContainerID) -> JsResult<Option<Array>> {
        let id: ContainerID = id.to_owned().try_into()?;
        let ans = self
            .0
            .get_path_to_container(&id)
            .map(|p| convert_container_path_to_js_value(&p));
        Ok(ans)
    }

    /// Evaluate JSONPath against a LoroDoc
    #[wasm_bindgen(js_name = "JSONPath")]
    pub fn json_path(&self, jsonpath: &str) -> JsResult<Array> {
        let ans = Array::new();
        for v in self
            .0
            .jsonpath(jsonpath)
            .map_err(|e| JsValue::from(e.to_string()))?
            .into_iter()
        {
            ans.push(&match v {
                ValueOrHandler::Handler(h) => handler_to_js_value(h, Some(self.0.clone())),
                ValueOrHandler::Value(v) => v.into(),
            });
        }
        Ok(ans)
    }

    /// Get the version vector of the current document state.
    ///
    /// If you checkout to a specific version, the version vector will change.
    #[inline(always)]
    pub fn version(&self) -> VersionVector {
        VersionVector(self.0.state_vv())
    }

    /// The doc only contains the history since this version
    ///
    /// This is empty if the doc is not shallow.
    ///
    /// The ops included by the shallow history start version vector are not in the doc.
    #[wasm_bindgen(js_name = "shallowSinceVV")]
    pub fn shallow_since_vv(&self) -> VersionVector {
        VersionVector(InternalVersionVector::from_im_vv(
            &self.0.shallow_since_vv(),
        ))
    }

    /// Check if the doc contains the full history.
    #[wasm_bindgen(js_name = "isShallow")]
    pub fn is_shallow(&self) -> bool {
        self.0.is_shallow()
    }

    /// The doc only contains the history since this version
    ///
    /// This is empty if the doc is not shallow.
    ///
    /// The ops included by the shallow history start frontiers are not in the doc.
    #[wasm_bindgen(js_name = "shallowSinceFrontiers")]
    pub fn shallow_since_frontiers(&self) -> JsIDs {
        frontiers_to_ids(&self.0.shallow_since_frontiers())
    }

    /// Get the version vector of the latest known version in OpLog.
    ///
    /// If you checkout to a specific version, this version vector will not change.
    #[wasm_bindgen(js_name = "oplogVersion")]
    pub fn oplog_version(&self) -> VersionVector {
        VersionVector(self.0.oplog_vv())
    }

    /// Get the [frontiers](https://loro.dev/docs/advanced/version_deep_dive) of the current document state.
    ///
    /// If you checkout to a specific version, this value will change.
    #[inline]
    pub fn frontiers(&self) -> JsIDs {
        frontiers_to_ids(&self.0.state_frontiers())
    }

    /// Get the [frontiers](https://loro.dev/docs/advanced/version_deep_dive) of the latest version in OpLog.
    ///
    /// If you checkout to a specific version, this value will not change.
    #[inline(always)]
    #[wasm_bindgen(js_name = "oplogFrontiers")]
    pub fn oplog_frontiers(&self) -> JsIDs {
        frontiers_to_ids(&self.0.oplog_frontiers())
    }

    /// Compare the version of the OpLog with the specified frontiers.
    ///
    /// This method is useful to compare the version by only a small amount of data.
    ///
    /// This method returns an integer indicating the relationship between the version of the OpLog (referred to as 'self')
    /// and the provided 'frontiers' parameter:
    ///
    /// - -1: The version of 'self' is either less than 'frontiers' or is non-comparable (parallel) to 'frontiers',
    ///        indicating that it is not definitively less than 'frontiers'.
    /// - 0: The version of 'self' is equal to 'frontiers'.
    /// - 1: The version of 'self' is greater than 'frontiers'.
    ///
    /// # Internal
    ///
    /// Frontiers cannot be compared without the history of the OpLog.
    ///
    #[wasm_bindgen(js_name = "cmpWithFrontiers")]
    pub fn cmp_with_frontiers(&self, frontiers: Vec<JsID>) -> JsResult<i32> {
        let frontiers = ids_to_frontiers(frontiers)?;
        Ok(match self.0.cmp_with_frontiers(&frontiers) {
            Ordering::Less => -1,
            Ordering::Greater => 1,
            Ordering::Equal => 0,
        })
    }

    /// Compare the ordering of two Frontiers.
    ///
    /// It's assumed that both Frontiers are included by the doc. Otherwise, an error will be thrown.
    ///
    /// Return value:
    ///
    /// - -1: a < b
    /// - 0: a == b
    /// - 1: a > b
    /// - undefined: a ∥ b: a and b are concurrent
    #[wasm_bindgen(js_name = "cmpFrontiers")]
    pub fn cmp_frontiers(&self, a: Vec<JsID>, b: Vec<JsID>) -> JsResult<JsPartialOrd> {
        let a = ids_to_frontiers(a)?;
        let b = ids_to_frontiers(b)?;
        let c = self
            .0
            .cmp_frontiers(&a, &b)
            .map_err(|e| JsError::new(&e.to_string()))?;
        if let Some(c) = c {
            let v: JsValue = match c {
                Ordering::Less => -1,
                Ordering::Greater => 1,
                Ordering::Equal => 0,
            }
            .into();
            Ok(v.into())
        } else {
            Ok(JsValue::UNDEFINED.into())
        }
    }

    /// Export the snapshot of current version.
    /// It includes all the history and the document state
    ///
    /// @deprecated Use `export({mode: "snapshot"})` instead
    #[wasm_bindgen(js_name = "exportSnapshot")]
    pub fn export_snapshot(&self) -> JsResult<Vec<u8>> {
        Ok(self.0.export_snapshot()?)
    }

    /// Export updates from the specific version to the current version
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// // get all updates of the doc
    /// const updates = doc.exportFrom();
    /// const version = doc.oplogVersion();
    /// text.insert(5, " World");
    /// // get updates from specific version to the latest version
    /// const updates2 = doc.exportFrom(version);
    /// ```
    #[wasm_bindgen(skip_typescript, js_name = "exportFrom")]
    pub fn export_from(&self, vv: Option<VersionVector>) -> JsResult<Vec<u8>> {
        if let Some(vv) = vv {
            // `version` may be null or undefined
            Ok(self.0.export_from(&vv.0))
        } else {
            Ok(self.0.export_from(&Default::default()))
        }
    }

    /// Export the document based on the specified ExportMode.
    ///
    /// @param mode - The export mode to use. Can be one of:
    ///   - `{ mode: "snapshot" }`: Export a full snapshot of the document.
    ///   - `{ mode: "update", from?: VersionVector }`: Export updates from the given version vector.
    ///   - `{ mode: "updates-in-range", spans: { id: ID, len: number }[] }`: Export updates within the specified ID spans.
    ///   - `{ mode: "shallow-snapshot", frontiers: Frontiers }`: Export a garbage-collected snapshot up to the given frontiers.
    ///
    /// @returns A byte array containing the exported data.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// doc.setPeerId("1");
    /// doc.getText("text").update("Hello World");
    ///
    /// // Export a full snapshot
    /// const snapshotBytes = doc.export({ mode: "snapshot" });
    ///
    /// // Export updates from a specific version
    /// const vv = doc.oplogVersion();
    /// doc.getText("text").update("Hello Loro");
    /// const updateBytes = doc.export({ mode: "update", from: vv });
    ///
    /// // Export a shallow snapshot that only includes the history since the frontiers
    /// const shallowBytes = doc.export({ mode: "shallow-snapshot", frontiers: doc.oplogFrontiers() });
    ///
    /// // Export updates within specific ID spans
    /// const spanBytes = doc.export({
    ///   mode: "updates-in-range",
    ///   spans: [{ id: { peer: "1", counter: 0 }, len: 10 }]
    /// });
    /// ```
    pub fn export(&self, mode: JsExportMode) -> JsResult<Vec<u8>> {
        let export_mode = js_to_export_mode(mode)
            .map_err(|e| JsValue::from_str(&format!("Invalid export mode. Error: {:?}", e)))?;
        Ok(self.0.export(export_mode)?)
    }

    /// Export updates in the given range in JSON format.
    #[wasm_bindgen(js_name = "exportJsonUpdates")]
    pub fn export_json_updates(
        &self,
        start_vv: Option<VersionVector>,
        end_vv: Option<VersionVector>,
    ) -> JsResult<JsJsonSchema> {
        let mut json_start_vv = Default::default();
        if let Some(vv) = start_vv {
            json_start_vv = vv.0;
        }
        let mut json_end_vv = self.oplog_version().0;
        if let Some(vv) = end_vv {
            json_end_vv = vv.0;
        }
        let json_schema = self.0.export_json_updates(&json_start_vv, &json_end_vv);
        let s = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        let v = json_schema
            .serialize(&s)
            .map_err(std::convert::Into::<JsValue>::into)?;
        Ok(v.into())
    }

    /// Import updates from the JSON format.
    ///
    /// only supports backward compatibility but not forward compatibility.
    #[wasm_bindgen(js_name = "importJsonUpdates")]
    pub fn import_json_updates(&self, json: JsJsonSchemaOrString) -> JsResult<JsImportStatus> {
        let json: JsValue = json.into();
        if JsValue::is_string(&json) {
            let json_str = json.as_string().unwrap();
            let status = self
                .0
                .import_json_updates(json_str.as_str())
                .map_err(JsValue::from)?;
            return Ok(import_status_to_js_value(status).into());
        }
        let json_schema: JsonSchema = serde_wasm_bindgen::from_value(json)?;
        let status = self.0.import_json_updates(json_schema)?;
        Ok(import_status_to_js_value(status).into())
    }

    /// Import snapshot or updates into current doc.
    ///
    /// Note:
    /// - Updates within the current version will be ignored
    /// - Updates with missing dependencies will be pending until the dependencies are received
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// // get all updates of the doc
    /// const updates = doc.export({ mode: "update" });
    /// const snapshot = doc.export({ mode: "snapshot" });
    /// const doc2 = new LoroDoc();
    /// // import snapshot
    /// doc2.import(snapshot);
    /// // or import updates
    /// doc2.import(updates);
    /// ```
    pub fn import(&self, update_or_snapshot: &[u8]) -> JsResult<JsImportStatus> {
        let status = self.0.import(update_or_snapshot)?;
        Ok(import_status_to_js_value(status).into())
    }

    /// Import a batch of updates.
    ///
    /// It's more efficient than importing updates one by one.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const updates = doc.export({ mode: "update" });
    /// const snapshot = doc.export({ mode: "snapshot" });
    /// const doc2 = new LoroDoc();
    /// doc2.importUpdateBatch([snapshot, updates]);
    /// ```
    #[wasm_bindgen(js_name = "importUpdateBatch")]
    pub fn import_update_batch(&mut self, data: Array) -> JsResult<()> {
        let data = data
            .iter()
            .map(|x| {
                let arr: Uint8Array = Uint8Array::new(&x);
                arr.to_vec()
            })
            .collect::<Vec<_>>();
        if data.is_empty() {
            return Ok(());
        }
        Ok(self.0.import_batch(&data)?)
    }

    /// Get the shallow json format of the document state.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// const tree = doc.getTree("tree");
    /// const map = doc.getMap("map");
    /// const shallowValue = doc.getShallowValue();
    /// /*
    /// {"list": ..., "tree": ..., "map": ...}
    ///  *\/
    /// console.log(shallowValue);
    /// ```
    #[wasm_bindgen(js_name = "getShallowValue")]
    pub fn get_shallow_value(&self) -> JsResult<JsValue> {
        let json = self.0.get_value();
        Ok(json.into())
    }

    /// Get the json format of the entire document state.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText, LoroMap } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, "Hello");
    /// const text = list.insertContainer(0, new LoroText());
    /// text.insert(0, "Hello");
    /// const map = list.insertContainer(1, new LoroMap());
    /// map.set("foo", "bar");
    /// /*
    /// {"list": ["Hello", {"foo": "bar"}]}
    ///  *\/
    /// console.log(doc.toJSON());
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsResult<JsValue> {
        let json = self.0.get_deep_value();
        Ok(json.into())
    }

    /// Subscribe to the changes of the loro document. The function will be called when the
    /// transaction is committed and after importing updates/snapshot from remote.
    ///
    /// Returns a subscription callback, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const sub = doc.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// text.insert(0, "Hello");
    /// // the events will be emitted when `commit()` is called.
    /// doc.commit();
    /// // unsubscribe
    /// sub();
    /// ```
    // TODO: convert event and event sub config
    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsValue {
        let observer = observer::Observer::new(f);
        let doc = self.0.clone();
        let sub = self.0.subscribe_root(Arc::new(move |e| {
            call_after_micro_task(observer.clone(), e, &doc)
            // call_subscriber(observer.clone(), e);
        }));
        subscription_to_js_function_callback(sub)
    }

    /// Subscribe the updates from local edits
    #[wasm_bindgen(js_name = "subscribeLocalUpdates", skip_typescript)]
    pub fn subscribe_local_updates(&self, f: js_sys::Function) -> JsValue {
        let observer = observer::Observer::new(f);
        let sub = self.0.subscribe_local_update(Box::new(move |e| {
            let arr = js_sys::Uint8Array::new_with_length(e.len() as u32);
            arr.copy_from(e);
            if let Err(e) = observer.call1(&arr.into()) {
                console_error!("Error: {:?}", e);
            }
            true
        }));

        subscription_to_js_function_callback(sub)
    }

    /// Debug the size of the history
    #[wasm_bindgen(js_name = "debugHistory")]
    pub fn debug_history(&self) {
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        console_log!("{:#?}", oplog.diagnose_size());
    }

    /// Get all of changes in the oplog.
    ///
    /// Note: this method is expensive when the oplog is large. O(n)
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const changes = doc.getAllChanges();
    ///
    /// for (let [peer, c] of changes.entries()){
    ///     console.log("peer: ", peer);
    ///     for (let change of c){
    ///         console.log("change: ", change);
    ///     }
    /// }
    /// ```
    #[wasm_bindgen(js_name = "getAllChanges")]
    pub fn get_all_changes(&self) -> JsChanges {
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        let mut changes: FxHashMap<PeerID, Vec<ChangeMeta>> = FxHashMap::default();
        oplog.change_store().visit_all_changes(&mut |c| {
            let change_meta = ChangeMeta {
                lamport: c.lamport(),
                length: c.atom_len() as u32,
                peer: c.peer().to_string(),
                counter: c.id().counter,
                deps: c
                    .deps()
                    .iter()
                    .map(|dep| StringID {
                        peer: dep.peer.to_string(),
                        counter: dep.counter,
                    })
                    .collect(),
                timestamp: c.timestamp() as f64,
                message: c.message().cloned(),
            };
            changes.entry(c.peer()).or_default().push(change_meta);
        });

        let ans = js_sys::Map::new();
        for (peer_id, changes) in changes {
            let row = js_sys::Array::new_with_length(changes.len() as u32);
            for (i, change) in changes.iter().enumerate() {
                row.set(i as u32, change.to_js());
            }
            ans.set(&peer_id.to_string().into(), &row);
        }

        let value: JsValue = ans.into();
        value.into()
    }

    /// Get the change that contains the specific ID
    #[wasm_bindgen(js_name = "getChangeAt")]
    pub fn get_change_at(&self, id: JsID) -> JsResult<JsChange> {
        let id = js_id_to_id(id)?;
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        let change = oplog
            .get_change_at(id)
            .ok_or_else(|| JsError::new(&format!("Change {:?} not found", id)))?;
        let change = ChangeMeta {
            lamport: change.lamport(),
            length: change.atom_len() as u32,
            peer: change.peer().to_string(),
            counter: change.id().counter,
            deps: change
                .deps()
                .iter()
                .map(|dep| StringID {
                    peer: dep.peer.to_string(),
                    counter: dep.counter,
                })
                .collect(),
            timestamp: change.timestamp() as f64,
            message: change.message().cloned(),
        };
        Ok(change.to_js().into())
    }

    /// Get the change of with specific peer_id and lamport <= given lamport
    #[wasm_bindgen(js_name = "getChangeAtLamport")]
    pub fn get_change_at_lamport(
        &self,
        peer_id: &str,
        lamport: u32,
    ) -> JsResult<JsChangeOrUndefined> {
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        let Some(change) =
            oplog.get_change_with_lamport_lte(peer_id.parse().unwrap_throw(), lamport)
        else {
            return Ok(JsValue::UNDEFINED.into());
        };

        let change = ChangeMeta {
            lamport: change.lamport(),
            length: change.atom_len() as u32,
            peer: change.peer().to_string(),
            counter: change.id().counter,
            deps: change
                .deps()
                .iter()
                .map(|dep| StringID {
                    peer: dep.peer.to_string(),
                    counter: dep.counter,
                })
                .collect(),
            timestamp: change.timestamp() as f64,
            message: change.message().cloned(),
        };
        Ok(change.to_js().into())
    }

    /// Get all ops of the change that contains the specific ID
    #[wasm_bindgen(js_name = "getOpsInChange")]
    pub fn get_ops_in_change(&self, id: JsID) -> JsResult<Vec<JsValue>> {
        let id = js_id_to_id(id)?;
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        let change = oplog
            .get_remote_change_at(id)
            .ok_or_else(|| JsError::new(&format!("Change {:?} not found", id)))?;
        let ops = change
            .ops()
            .iter()
            .map(|op| serde_wasm_bindgen::to_value(op).unwrap())
            .collect::<Vec<_>>();
        Ok(ops)
    }

    /// Convert frontiers to a version vector
    ///
    /// Learn more about frontiers and version vector [here](https://loro.dev/docs/advanced/version_deep_dive)
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const frontiers = doc.frontiers();
    /// const version = doc.frontiersToVV(frontiers);
    /// ```
    #[wasm_bindgen(js_name = "frontiersToVV")]
    pub fn frontiers_to_vv(&self, frontiers: Vec<JsID>) -> JsResult<VersionVector> {
        let frontiers = ids_to_frontiers(frontiers)?;
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        oplog
            .dag()
            .frontiers_to_vv(&frontiers)
            .map(VersionVector)
            .ok_or_else(|| JsError::new("Frontiers not found").into())
    }

    /// Convert a version vector to frontiers
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const version = doc.version();
    /// const frontiers = doc.vvToFrontiers(version);
    /// ```
    #[wasm_bindgen(js_name = "vvToFrontiers")]
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> JsResult<JsIDs> {
        let f = self
            .0
            .oplog()
            .try_lock()
            .unwrap()
            .dag()
            .vv_to_frontiers(&vv.0);
        Ok(frontiers_to_ids(&f))
    }

    /// Get the value or container at the given path
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("key", 1);
    /// console.log(doc.getByPath("map/key")); // 1
    /// console.log(doc.getByPath("map"));     // LoroMap
    /// ```
    #[wasm_bindgen(js_name = "getByPath")]
    pub fn get_by_path(&self, path: &str) -> JsValueOrContainerOrUndefined {
        let ans = self.0.get_by_str_path(path);
        let v: JsValue = match ans {
            Some(ValueOrHandler::Handler(h)) => handler_to_js_value(h, Some(self.0.clone())),
            Some(ValueOrHandler::Value(v)) => v.into(),
            None => JsValue::UNDEFINED,
        };
        v.into()
    }

    /// Get the absolute position of the given Cursor
    ///
    /// @example
    /// ```ts
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "123");
    /// const pos0 = text.getCursor(0, 0);
    /// {
    ///    const ans = doc.getCursorPos(pos0!);
    ///    expect(ans.offset).toBe(0);
    /// }
    /// text.insert(0, "1");
    /// {
    ///    const ans = doc.getCursorPos(pos0!);
    ///    expect(ans.offset).toBe(1);
    /// }
    /// ```
    pub fn getCursorPos(&self, cursor: &Cursor) -> JsResult<JsCursorQueryAns> {
        let ans = self
            .0
            .query_pos(&cursor.pos)
            .map_err(|e| JsError::new(&e.to_string()))?;

        let obj = Object::new();
        let update = ans.update.map(|u| Cursor { pos: u });
        if let Some(update) = update {
            let update_value: JsValue = update.into();
            Reflect::set(&obj, &JsValue::from_str("update"), &update_value)?;
        }
        Reflect::set(
            &obj,
            &JsValue::from_str("offset"),
            &JsValue::from(ans.current.pos),
        )?;
        Reflect::set(
            &obj,
            &JsValue::from_str("side"),
            &JsValue::from(ans.current.side.to_i32()),
        )?;
        Ok(JsValue::from(obj).into())
    }

    /// Gets container IDs modified in the given ID range.
    ///
    /// **NOTE:** This method will implicitly commit.
    ///
    /// This method identifies which containers were affected by changes in a given range of operations.
    /// It can be used together with `doc.travelChangeAncestors()` to analyze the history of changes
    /// and determine which containers were modified by each change.
    ///
    /// @param id - The starting ID of the change range
    /// @param len - The length of the change range to check
    /// @returns An array of container IDs that were modified in the given range
    pub fn getChangedContainersIn(&self, id: JsID, len: usize) -> JsResult<Vec<JsContainerID>> {
        let id = js_id_to_id(id)?;
        Ok(self
            .0
            .get_changed_containers_in(id, len)
            .into_iter()
            .map(|cid| {
                let v: JsValue = (&cid).into();
                v.into()
            })
            .collect())
    }
}

#[allow(unused)]
fn call_subscriber(ob: observer::Observer, e: DiffEvent, doc: &Arc<LoroDocInner>) {
    // We convert the event to js object here, so that we don't need to worry about GC.
    // In the future, when FinalizationRegistry[1] is stable, we can use `--weak-ref`[2] feature
    // in wasm-bindgen to avoid this.
    //
    // [1]: https://caniuse.com/?search=FinalizationRegistry
    // [2]: https://rustwasm.github.io/wasm-bindgen/reference/weak-references.html
    let event = diff_event_to_js_value(e, doc);
    if let Err(e) = ob.call1(&event) {
        throw_error_after_micro_task(e);
    }
}

#[allow(unused)]
fn call_after_micro_task(ob: observer::Observer, event: DiffEvent, doc: &Arc<LoroDocInner>) {
    let promise = Promise::resolve(&JsValue::NULL);
    type C = Closure<dyn FnMut(JsValue)>;
    let drop_handler: Rc<RefCell<Option<C>>> = Rc::new(RefCell::new(None));
    let copy = drop_handler.clone();
    let event = diff_event_to_js_value(event, doc);
    let closure = Closure::once(move |_: JsValue| {
        let ans = ob.call1(&event);
        drop(copy);
        if let Err(e) = ans {
            throw_error_after_micro_task(e)
        }
    });

    let _ = promise.then(&closure);
    drop_handler.borrow_mut().replace(closure);
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

fn diff_event_to_js_value(event: DiffEvent, doc: &Arc<LoroDocInner>) -> JsValue {
    let obj = js_sys::Object::new();
    Reflect::set(&obj, &"by".into(), &event.event_meta.by.to_string().into()).unwrap();
    let origin: &str = &event.event_meta.origin;
    Reflect::set(&obj, &"origin".into(), &JsValue::from_str(origin)).unwrap();
    if let Some(t) = event.current_target.as_ref() {
        Reflect::set(&obj, &"currentTarget".into(), &t.to_string().into()).unwrap();
    }

    let events = js_sys::Array::new_with_length(event.events.len() as u32);
    for (i, &event) in event.events.iter().enumerate() {
        events.set(i as u32, container_diff_to_js_value(event, doc));
    }

    Reflect::set(&obj, &"events".into(), &events.into()).unwrap();
    obj.into()
}

/// /**
///  * The concrete event of Loro.
///  */
/// export interface LoroEvent {
///   /**
///    * The container ID of the event's target.
///    */
///   target: ContainerID;
///   diff: Diff;
///   /**
///    * The absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
///    */
///   path: Path;
/// }
///
fn container_diff_to_js_value(
    event: &loro_internal::ContainerDiff,
    doc: &Arc<LoroDocInner>,
) -> JsValue {
    let obj = js_sys::Object::new();
    Reflect::set(&obj, &"target".into(), &event.id.to_string().into()).unwrap();
    Reflect::set(&obj, &"diff".into(), &resolved_diff_to_js(&event.diff, doc)).unwrap();
    Reflect::set(
        &obj,
        &"path".into(),
        &convert_container_path_to_js_value(&event.path),
    )
    .unwrap();
    obj.into()
}

fn convert_container_path_to_js_value(path: &[(ContainerID, Index)]) -> Array {
    let arr = Array::new_with_length(path.len() as u32);
    for (i, p) in path.iter().enumerate() {
        arr.set(i as u32, p.1.clone().into());
    }
    arr
}

/// The handler of a text container. It supports rich text CRDT.
///
/// Learn more at https://loro.dev/docs/tutorial/text
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroText {
    handler: TextHandler,
    doc: Option<Arc<LoroDocInner>>,
    delta_cache: Option<(usize, JsValue)>,
}

#[derive(Serialize, Deserialize)]
struct MarkRange {
    start: usize,
    end: usize,
}

#[wasm_bindgen]
impl LoroText {
    /// Create a new detached LoroText (not attached to any LoroDoc).
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            handler: TextHandler::new_detached(),
            doc: None,
            delta_cache: None,
        }
    }

    /// "Text"
    pub fn kind(&self) -> JsTextStr {
        JsValue::from_str("Text").into()
    }

    /// Iterate each text span(internal storage unit)
    ///
    /// The callback function will be called for each span in the text.
    /// If the callback returns `false`, the iteration will stop.
    ///
    /// Limitation: you cannot access or alter the doc state when iterating (this is for performance consideration).
    /// If you need to access or alter the doc state, please use `toString` instead.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.iter((str) => (console.log(str), true));
    /// ```
    pub fn iter(&self, callback: &js_sys::Function) {
        let context = JsValue::NULL;
        self.handler.iter(|c| {
            let result = callback.call1(&context, &JsValue::from(c)).unwrap();
            result.as_bool().unwrap()
        })
    }

    /// Update the current text to the target text.
    ///
    /// It will calculate the minimal difference and apply it to the current text.
    /// It uses Myers' diff algorithm to compute the optimal difference.
    ///
    /// This could take a long time for large texts (e.g. > 50_000 characters).
    /// In that case, you should use `updateByLine` instead.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.update("Hello World");
    /// console.log(text.toString()); // "Hello World"
    /// ```
    ///
    #[wasm_bindgen(skip_typescript)]
    pub fn update(&self, text: &str, options: JsValue) -> JsResult<()> {
        let options = if options.is_null() || options.is_undefined() {
            UpdateOptions {
                timeout_ms: None,
                use_refined_diff: true,
            }
        } else {
            let opts = match js_sys::Object::try_from(&options) {
                Some(o) => o,
                None => return Err(JsError::new("Invalid options").into()),
            };
            UpdateOptions {
                timeout_ms: js_sys::Reflect::get(opts, &"timeoutMs".into())
                    .ok()
                    .and_then(|v| v.as_f64()),
                use_refined_diff: js_sys::Reflect::get(opts, &"useRefinedDiff".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            }
        };
        self.handler
            .update(text, options)
            .map_err(|_| JsError::new("Update timeout").into())
    }

    /// Update the current text to the target text, the difference is calculated line by line.
    ///
    /// It uses Myers' diff algorithm to compute the optimal difference.
    #[wasm_bindgen(js_name = "updateByLine", skip_typescript)]
    pub fn update_by_line(&self, text: &str, options: JsValue) -> JsResult<()> {
        let options = if options.is_null() || options.is_undefined() {
            UpdateOptions {
                timeout_ms: None,
                use_refined_diff: true,
            }
        } else {
            let opts = match js_sys::Object::try_from(&options) {
                Some(o) => o,
                None => return Err(JsError::new("Invalid options").into()),
            };
            UpdateOptions {
                timeout_ms: js_sys::Reflect::get(opts, &"timeoutMs".into())
                    .ok()
                    .and_then(|v| v.as_f64()),
                use_refined_diff: js_sys::Reflect::get(opts, &"useRefinedDiff".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            }
        };
        self.handler
            .update_by_line(text, options)
            .map_err(|_| JsError::new("Update timeout").into())
    }

    /// Insert the string at the given index (utf-16 index).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// ```
    pub fn insert(&mut self, index: usize, content: &str) -> JsResult<()> {
        self.handler.insert(index, content)?;
        Ok(())
    }

    /// Get a string slice (utf-16 index).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.slice(0, 2); // "He"
    /// ```
    pub fn slice(&mut self, start_index: usize, end_index: usize) -> JsResult<String> {
        match self.handler.slice(start_index, end_index) {
            Ok(x) => Ok(x),
            Err(x) => Err(x.into()),
        }
    }

    /// Get the character at the given position (utf-16 index).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.charAt(0); // "H"
    /// ```
    #[wasm_bindgen(js_name = "charAt")]
    pub fn char_at(&mut self, pos: usize) -> JsResult<char> {
        match self.handler.char_at(pos) {
            Ok(x) => Ok(x),
            Err(x) => Err(x.into()),
        }
    }

    /// Delete and return the string at the given range and insert a string at the same position (utf-16 index).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.splice(2, 3, "llo"); // "llo"
    /// ```
    pub fn splice(&mut self, pos: usize, len: usize, s: &str) -> JsResult<String> {
        match self.handler.splice(pos, len, s) {
            Ok(x) => Ok(x),
            Err(x) => Err(x.into()),
        }
    }

    /// Insert some string at utf-8 index.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insertUtf8(0, "Hello");
    /// ```
    #[wasm_bindgen(js_name = "insertUtf8")]
    pub fn insert_utf8(&mut self, index: usize, content: &str) -> JsResult<()> {
        self.handler.insert_utf8(index, content)?;
        Ok(())
    }

    /// Delete elements from index to index + len (utf-16 index).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.delete(1, 3);
    /// const s = text.toString();
    /// console.log(s); // "Ho"
    /// ```
    pub fn delete(&mut self, index: usize, len: usize) -> JsResult<()> {
        self.handler.delete(index, len)?;
        Ok(())
    }

    /// Delete elements from index to utf-8 index + len
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insertUtf8(0, "Hello");
    /// text.deleteUtf8(1, 3);
    /// const s = text.toString();
    /// console.log(s); // "Ho"
    /// ```
    #[wasm_bindgen(js_name = "deleteUtf8")]
    pub fn delete_utf8(&mut self, index: usize, len: usize) -> JsResult<()> {
        self.handler.delete_utf8(index, len)?;
        Ok(())
    }

    /// Mark a range of text with a key and a value (utf-16 index).
    ///
    /// > You should call `configTextStyle` before using `mark` and `unmark`.
    ///
    /// You can use it to create a highlight, make a range of text bold, or add a link to a range of text.
    ///
    /// Note: this is not suitable for unmergeable annotations like comments.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// doc.configTextStyle({bold: {expand: "after"}});
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// ```
    pub fn mark(&self, range: JsRange, key: &str, value: JsValue) -> Result<(), JsError> {
        let range: MarkRange = serde_wasm_bindgen::from_value(range.into())?;
        let value: LoroValue = LoroValue::from(value);
        self.handler.mark(range.start, range.end, key, value)?;
        Ok(())
    }

    /// Unmark a range of text with a key and a value (utf-16 index).
    ///
    /// > You should call `configTextStyle` before using `mark` and `unmark`.
    ///
    /// You can use it to remove highlights, bolds or links
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// doc.configTextStyle({bold: {expand: "after"}});
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// text.unmark({ start: 0, end: 5 }, "bold");
    /// ```
    pub fn unmark(&self, range: JsRange, key: &str) -> Result<(), JsValue> {
        // Internally, this may be marking with null or deleting all the marks with key in the range entirely.
        let range: MarkRange = serde_wasm_bindgen::from_value(range.into())?;
        self.handler.unmark(range.start, range.end, key)?;
        Ok(())
    }

    /// Convert the text to a string
    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.handler.get_value().as_string().unwrap().to_string()
    }

    /// Get the text in [Delta](https://quilljs.com/docs/delta/) format.
    ///
    /// The returned value will include the rich text information.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// doc.configTextStyle({bold: {expand: "after"}});
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// console.log(text.toDelta());  // [ { insert: 'Hello', attributes: { bold: true } } ]
    /// ```
    #[wasm_bindgen(js_name = "toDelta")]
    pub fn to_delta(&mut self) -> JsStringDelta {
        let version = self.handler.version_id();
        if let Some((v, delta)) = self.delta_cache.as_ref() {
            if *v == version {
                return delta.clone().into();
            }
        }

        let delta = self.handler.get_richtext_value();
        let value: JsValue = delta.into();
        let ans: JsStringDelta = value.clone().into();
        self.delta_cache = Some((version, value));
        ans
    }

    /// Get the container id of the text.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Get the length of text (utf-16 length).
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.handler.len_utf16()
    }

    /// Subscribe to the changes of the text.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// returns a subscription callback, which can be used to unsubscribe.
    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<JsValue> {
        let observer = observer::Observer::new(f);
        let doc = self
            .doc
            .clone()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = doc.clone();
        let ans = doc.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );

        Ok(subscription_to_js_function_callback(ans))
    }

    /// Change the state of this text by delta.
    ///
    /// If a delta item is `insert`, it should include all the attributes of the inserted text.
    /// Loro's rich text CRDT may make the inserted text inherit some styles when you use
    /// `insert` method directly. However, when you use `applyDelta` if some attributes are
    /// inherited from CRDT but not included in the delta, they will be removed.
    ///
    /// Another special property of `applyDelta` is if you format an attribute for ranges out of
    /// the text length, Loro will insert new lines to fill the gap first. It's useful when you
    /// build the binding between Loro and rich text editors like Quill, which might assume there
    /// is always a newline at the end of the text implicitly.
    ///
    /// @example
    /// ```ts
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// doc.configTextStyle({bold: {expand: "after"}});
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// const delta = text.toDelta();
    /// const text2 = doc.getText("text2");
    /// text2.applyDelta(delta);
    /// expect(text2.toDelta()).toStrictEqual(delta);
    /// ```
    #[wasm_bindgen(js_name = "applyDelta")]
    pub fn apply_delta(&self, delta: JsDelta) -> JsResult<()> {
        let delta: Vec<TextDelta> = serde_wasm_bindgen::from_value(delta.into())?;
        self.handler.apply_delta(&delta)?;
        Ok(())
    }

    /// Get the parent container.
    ///
    /// - The parent of the root is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = self.handler.parent() {
            handler_to_js_value(p, self.doc.clone()).into()
        } else {
            JsContainerOrUndefined::from(JsValue::UNDEFINED)
        }
    }

    /// Whether the container is attached to a LoroDoc.
    ///
    /// If it's detached, the operations on the container will not be persisted.
    #[wasm_bindgen(js_name = "isAttached")]
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Get the attached container associated with this.
    ///
    /// Returns an attached `Container` that is equal to this or created by this; otherwise, it returns `undefined`.
    #[wasm_bindgen(js_name = "getAttached")]
    pub fn get_attached(&self) -> JsLoroTextOrUndefined {
        if self.is_attached() {
            let value: JsValue = self.clone().into();
            return value.into();
        }

        if let Some(h) = self.handler.get_attached() {
            handler_to_js_value(Handler::Text(h), self.doc.clone()).into()
        } else {
            JsValue::UNDEFINED.into()
        }
    }

    /// Get the cursor at the given position.
    ///
    /// - The first argument is the position (utf16-index).
    /// - The second argument is the side: `-1` for left, `0` for middle, `1` for right.
    #[wasm_bindgen(skip_typescript)]
    pub fn getCursor(&self, pos: usize, side: JsSide) -> Option<Cursor> {
        let mut side_value = Side::Middle;
        if side.is_truthy() {
            let num = side.as_f64().expect("Side must be -1 | 0 | 1");
            side_value = Side::from_i32(num as i32).expect("Side must be -1 | 0 | 1");
        }
        self.handler
            .get_cursor(pos, side_value)
            .map(|pos| Cursor { pos })
    }

    /// Push a string to the end of the text.
    pub fn push(&mut self, s: &str) -> JsResult<()> {
        self.handler.push_str(s)?;
        Ok(())
    }

    /// Get the editor of the text at the given position.
    pub fn getEditorOf(&self, pos: usize) -> Option<JsStrPeerID> {
        self.handler
            .get_cursor(pos, Side::Middle)
            .map(|x| peer_id_to_js(x.id.unwrap().peer))
    }
}

impl Default for LoroText {
    fn default() -> Self {
        Self::new()
    }
}

/// The handler of a map container.
///
/// Learn more at https://loro.dev/docs/tutorial/map
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroMap {
    handler: MapHandler,
    doc: Option<Arc<LoroDocInner>>,
}

#[wasm_bindgen]
impl LoroMap {
    /// Create a new detached LoroMap (not attached to any LoroDoc).
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            handler: MapHandler::new_detached(),
            doc: None,
        }
    }

    /// "Map"
    pub fn kind(&self) -> JsMapStr {
        JsValue::from_str("Map").into()
    }

    /// Set the key with the value.
    ///
    /// If the value of the key is exist, the old value will be updated.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.set("foo", "baz");
    /// ```
    #[wasm_bindgen(js_name = "set", skip_typescript)]
    pub fn insert(&mut self, key: &str, value: JsLoroValue) -> JsResult<()> {
        let v: JsValue = value.into();
        self.handler.insert(key, v)?;
        Ok(())
    }

    /// Remove the key from the map.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.delete("foo");
    /// ```
    pub fn delete(&mut self, key: &str) -> JsResult<()> {
        self.handler.delete(key)?;
        Ok(())
    }

    /// Get the value of the key. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// The object/value returned is a new js object/value each time because it need to cross
    /// the WASM boundary.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const bar = map.get("foo");
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn get(&self, key: &str) -> JsValueOrContainerOrUndefined {
        let v = self.handler.get_(key);
        (match v {
            Some(ValueOrHandler::Handler(c)) => handler_to_js_value(c, self.doc.clone()),
            Some(ValueOrHandler::Value(v)) => v.into(),
            None => JsValue::UNDEFINED,
        })
        .into()
    }

    /// Get the value of the key. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// The object returned is a new js object each time because it need to cross
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const bar = map.get("foo");
    /// ```
    #[wasm_bindgen(js_name = "getOrCreateContainer", skip_typescript)]
    pub fn get_or_create_container(&self, key: &str, child: JsContainer) -> JsResult<JsContainer> {
        let child = convert::js_to_container(child)?;
        let handler = self
            .handler
            .get_or_create_container(key, child.to_handler())?;
        Ok(handler_to_js_value(handler, self.doc.clone()).into())
    }

    /// Get the keys of the map.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.set("baz", "bar");
    /// const keys = map.keys(); // ["foo", "baz"]
    /// ```
    pub fn keys(&self) -> Vec<JsValue> {
        let mut ans = Vec::with_capacity(self.handler.len());
        self.handler.for_each(|k, _| {
            ans.push(k.to_string().into());
        });
        ans
    }

    /// Get the values of the map. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.set("baz", "bar");
    /// const values = map.values(); // ["bar", "bar"]
    /// ```
    pub fn values(&self) -> Vec<JsValue> {
        let mut ans: Vec<JsValue> = Vec::with_capacity(self.handler.len());
        self.handler.for_each(|_, v| {
            ans.push(loro_value_to_js_value_or_container(v, self.doc.clone()));
        });
        ans
    }

    /// Get the entries of the map. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.set("baz", "bar");
    /// const entries = map.entries(); // [["foo", "bar"], ["baz", "bar"]]
    /// ```
    pub fn entries(&self) -> Vec<MapEntry> {
        let mut ans: Vec<MapEntry> = Vec::with_capacity(self.handler.len());
        self.handler.for_each(|k, v| {
            let array = Array::new();
            array.push(&k.to_string().into());
            array.push(&loro_value_to_js_value_or_container(v, self.doc.clone()));
            let v: JsValue = array.into();
            ans.push(v.into());
        });
        ans
    }

    /// The container id of this handler.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Get the keys and the values. If the type of value is a child container,
    /// it will be resolved recursively.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const text = map.setContainer("text", new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(map.toJSON());  // {"foo": "bar", "text": "Hello"}
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsValue {
        self.handler.get_deep_value().into()
    }

    /// Set the key with a container.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const text = map.setContainer("text", new LoroText());
    /// const list = map.setContainer("list", new LoroText());
    /// ```
    #[wasm_bindgen(js_name = "setContainer", skip_typescript)]
    pub fn insert_container(&mut self, key: &str, child: JsContainer) -> JsResult<JsContainer> {
        let child = convert::js_to_container(child)?;
        let c = self.handler.insert_container(key, child.to_handler())?;
        Ok(handler_to_js_value(c, self.doc.clone()).into())
    }

    /// Subscribe to the changes of the map.
    ///
    /// Returns a subscription callback, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// @param {Listener} f - Event listener
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// map.set("foo", "bar");
    /// doc.commit();
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<JsValue> {
        let observer = observer::Observer::new(f);
        let doc = self
            .doc
            .clone()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = doc.clone();
        let sub = doc.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );

        Ok(subscription_to_js_function_callback(sub))
    }

    /// Get the size of the map.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// console.log(map.size);   // 1
    /// ```
    #[wasm_bindgen(js_name = "size", method, getter)]
    pub fn size(&self) -> usize {
        self.handler.len()
    }

    /// Get the parent container.
    ///
    /// - The parent container of the root tree is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = self.handler.parent() {
            handler_to_js_value(p, self.doc.clone()).into()
        } else {
            JsContainerOrUndefined::from(JsValue::UNDEFINED)
        }
    }

    /// Whether the container is attached to a document.
    ///
    /// If it's detached, the operations on the container will not be persisted.
    #[wasm_bindgen(js_name = "isAttached")]
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Get the attached container associated with this.
    ///
    /// Returns an attached `Container` that equals to this or created by this, otherwise `undefined`.
    #[wasm_bindgen(js_name = "getAttached")]
    pub fn get_attached(&self) -> JsLoroMapOrUndefined {
        if self.is_attached() {
            let value: JsValue = self.clone().into();
            return value.into();
        }

        let Some(h) = self.handler.get_attached() else {
            return JsValue::UNDEFINED.into();
        };
        handler_to_js_value(Handler::Map(h), self.doc.clone()).into()
    }

    /// Delete all key-value pairs in the map.
    pub fn clear(&self) -> JsResult<()> {
        self.handler.clear()?;
        Ok(())
    }

    /// Get the peer id of the last editor on the given entry
    pub fn getLastEditor(&self, key: &str) -> Option<JsStrPeerID> {
        self.handler
            .get_last_editor(key)
            .map(|x| JsValue::from_str(&x.to_string()).into())
    }
}

impl Default for LoroMap {
    fn default() -> Self {
        Self::new()
    }
}

/// The handler of a list container.
///
/// Learn more at https://loro.dev/docs/tutorial/list
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroList {
    handler: ListHandler,
    doc: Option<Arc<LoroDocInner>>,
}

#[wasm_bindgen]
impl LoroList {
    /// Create a new detached LoroList (not attached to any LoroDoc).
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            handler: ListHandler::new_detached(),
            doc: None,
        }
    }

    /// "List"
    pub fn kind(&self) -> JsListStr {
        JsValue::from_str("List").into()
    }

    /// Insert a value at index.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// console.log(list.value);  // [100, "foo", true];
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn insert(&mut self, index: usize, value: JsLoroValue) -> JsResult<()> {
        let v: JsValue = value.into();
        self.handler.insert(index, v)?;
        Ok(())
    }

    /// Delete elements from index to index + len.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.delete(0, 1);
    /// console.log(list.value);  // []
    /// ```
    pub fn delete(&mut self, index: usize, len: usize) -> JsResult<()> {
        self.handler.delete(index, len)?;
        Ok(())
    }

    /// Get the value at the index. If the value is a container, the corresponding handler will be returned.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// console.log(list.get(0));  // 100
    /// console.log(list.get(1));  // undefined
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn get(&self, index: usize) -> JsValueOrContainerOrUndefined {
        let Some(v) = self.handler.get_(index) else {
            return JsValue::UNDEFINED.into();
        };

        (match v {
            ValueOrHandler::Value(v) => v.into(),
            ValueOrHandler::Handler(h) => handler_to_js_value(h, self.doc.clone()),
        })
        .into()
    }

    /// Get the id of this container.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Get elements of the list. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// list.insertContainer(3, new LoroText());
    /// console.log(list.value);  // [100, "foo", true, LoroText];
    /// ```
    #[wasm_bindgen(js_name = "toArray", method, skip_typescript)]
    pub fn to_array(&mut self) -> Vec<JsValueOrContainer> {
        let mut arr: Vec<JsValueOrContainer> = Vec::with_capacity(self.length());
        self.handler.for_each(|x| {
            arr.push(match x {
                ValueOrHandler::Value(v) => {
                    let v: JsValue = v.into();
                    v.into()
                }
                ValueOrHandler::Handler(h) => {
                    let v: JsValue = handler_to_js_value(h, self.doc.clone());
                    v.into()
                }
            });
        });
        arr
    }

    /// Get elements of the list. If the type of a element is a container, it will be
    /// resolved recursively.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.toJSON());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsValue {
        let value = self.handler.get_deep_value();
        value.into()
    }

    /// Insert a container at the index.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.toJSON());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "insertContainer", skip_typescript)]
    pub fn insert_container(&mut self, index: usize, child: JsContainer) -> JsResult<JsContainer> {
        let child = js_to_container(child)?;
        let c = self.handler.insert_container(index, child.to_handler())?;
        Ok(handler_to_js_value(c, self.doc.clone()).into())
    }

    #[wasm_bindgen(js_name = "pushContainer", skip_typescript)]
    pub fn push_container(&mut self, child: JsContainer) -> JsResult<JsContainer> {
        self.insert_container(self.length(), child)
    }

    /// Subscribe to the changes of the list.
    ///
    /// Returns a subscription callback, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// list.insert(0, 100);
    /// doc.commit();
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<JsValue> {
        let observer = observer::Observer::new(f);
        let doc = self
            .doc
            .clone()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = doc.clone();
        let sub = doc.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );
        Ok(subscription_to_js_function_callback(sub))
    }

    /// Get the length of list.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// console.log(list.length);  // 3
    /// ```
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.handler.len()
    }

    /// Get the parent container.
    ///
    /// - The parent container of the root tree is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = self.handler.parent() {
            handler_to_js_value(p, self.doc.clone()).into()
        } else {
            JsContainerOrUndefined::from(JsValue::UNDEFINED)
        }
    }

    /// Whether the container is attached to a document.
    ///
    /// If it's detached, the operations on the container will not be persisted.
    #[wasm_bindgen(js_name = "isAttached")]
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Get the attached container associated with this.
    ///
    /// Returns an attached `Container` that equals to this or created by this, otherwise `undefined`.
    #[wasm_bindgen(js_name = "getAttached")]
    pub fn get_attached(&self) -> JsLoroListOrUndefined {
        if self.is_attached() {
            let value: JsValue = self.clone().into();
            return value.into();
        }

        if let Some(h) = self.handler.get_attached() {
            handler_to_js_value(Handler::List(h), self.doc.clone()).into()
        } else {
            JsValue::UNDEFINED.into()
        }
    }

    /// Get the cursor at the position.
    ///
    /// - The first argument is the position .
    /// - The second argument is the side: `-1` for left, `0` for middle, `1` for right.
    #[wasm_bindgen(skip_typescript)]
    pub fn getCursor(&self, pos: usize, side: JsSide) -> Option<Cursor> {
        let mut side_value = Side::Middle;
        if side.is_truthy() {
            let num = side.as_f64().expect("Side must be -1 | 0 | 1");
            side_value = Side::from_i32(num as i32).expect("Side must be -1 | 0 | 1");
        }
        self.handler
            .get_cursor(pos, side_value)
            .map(|pos| Cursor { pos })
    }

    /// Push a value to the end of the list.
    #[wasm_bindgen(skip_typescript)]
    pub fn push(&self, value: JsLoroValue) -> JsResult<()> {
        let v: JsValue = value.into();
        self.handler.push(v)?;
        Ok(())
    }

    /// Pop a value from the end of the list.
    pub fn pop(&self) -> JsResult<Option<JsLoroValue>> {
        let v = self.handler.pop()?;
        if let Some(v) = v {
            let v: JsValue = v.into();
            Ok(Some(v.into()))
        } else {
            Ok(None)
        }
    }

    /// Delete all elements in the list.
    pub fn clear(&self) -> JsResult<()> {
        self.handler.clear()?;
        Ok(())
    }

    pub fn getIdAt(&self, pos: usize) -> Option<JsID> {
        self.handler.get_id_at(pos).map(|x| id_to_js(&x).into())
    }
}

impl Default for LoroList {
    fn default() -> Self {
        Self::new()
    }
}

/// The handler of a list container.
///
/// Learn more at https://loro.dev/docs/tutorial/list
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroMovableList {
    handler: MovableListHandler,
    doc: Option<Arc<LoroDocInner>>,
}

impl Default for LoroMovableList {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl LoroMovableList {
    /// Create a new detached LoroMovableList (not attached to any LoroDoc).
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            handler: MovableListHandler::new_detached(),
            doc: None,
        }
    }

    /// "MovableList"
    pub fn kind(&self) -> JsMovableListStr {
        JsValue::from_str("MovableList").into()
    }

    /// Insert a value at index.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// console.log(list.value);  // [100, "foo", true];
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn insert(&mut self, index: usize, value: JsLoroValue) -> JsResult<()> {
        let v: JsValue = value.into();
        self.handler.insert(index, v)?;
        Ok(())
    }

    /// Delete elements from index to index + len.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.delete(0, 1);
    /// console.log(list.value);  // []
    /// ```
    pub fn delete(&mut self, index: usize, len: usize) -> JsResult<()> {
        self.handler.delete(index, len)?;
        Ok(())
    }

    /// Get the value at the index. If the value is a container, the corresponding handler will be returned.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// console.log(list.get(0));  // 100
    /// console.log(list.get(1));  // undefined
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn get(&self, index: usize) -> JsValueOrContainerOrUndefined {
        let Some(v) = self.handler.get_(index) else {
            return JsValue::UNDEFINED.into();
        };

        (match v {
            ValueOrHandler::Value(v) => v.into(),
            ValueOrHandler::Handler(h) => handler_to_js_value(h, self.doc.clone()),
        })
        .into()
    }

    /// Get the id of this container.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Get elements of the list. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// list.insertContainer(3, new LoroText());
    /// console.log(list.value);  // [100, "foo", true, LoroText];
    /// ```
    #[wasm_bindgen(js_name = "toArray", method, skip_typescript)]
    pub fn to_array(&mut self) -> Vec<JsValueOrContainer> {
        let mut arr: Vec<JsValueOrContainer> = Vec::with_capacity(self.length());
        self.handler.for_each(|x| {
            arr.push(match x {
                ValueOrHandler::Value(v) => {
                    let v: JsValue = v.into();
                    v.into()
                }
                ValueOrHandler::Handler(h) => {
                    let v: JsValue = handler_to_js_value(h, self.doc.clone());
                    v.into()
                }
            });
        });
        arr
    }

    /// Get elements of the list. If the type of a element is a container, it will be
    /// resolved recursively.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.toJSON());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsValue {
        let value = self.handler.get_deep_value();
        value.into()
    }

    /// Insert a container at the index.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc, LoroText } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.toJSON());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "insertContainer", skip_typescript)]
    pub fn insert_container(&mut self, index: usize, child: JsContainer) -> JsResult<JsContainer> {
        let child = js_to_container(child)?;
        let c = self.handler.insert_container(index, child.to_handler())?;
        Ok(handler_to_js_value(c, self.doc.clone()).into())
    }

    /// Push a container to the end of the list.
    #[wasm_bindgen(js_name = "pushContainer", skip_typescript)]
    pub fn push_container(&mut self, child: JsContainer) -> JsResult<JsContainer> {
        self.insert_container(self.length(), child)
    }

    /// Subscribe to the changes of the list.
    ///
    /// Returns a subscription callback, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// list.insert(0, 100);
    /// doc.commit();
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<JsValue> {
        let observer = observer::Observer::new(f);
        let loro = self
            .doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = loro.clone();
        let sub = loro.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );
        Ok(subscription_to_js_function_callback(sub))
    }

    /// Get the length of list.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// console.log(list.length);  // 3
    /// ```
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.handler.len()
    }

    /// Get the parent container.
    ///
    /// - The parent container of the root tree is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = self.handler.parent() {
            handler_to_js_value(p, self.doc.clone()).into()
        } else {
            JsContainerOrUndefined::from(JsValue::UNDEFINED)
        }
    }

    /// Whether the container is attached to a document.
    ///
    /// If it's detached, the operations on the container will not be persisted.
    #[wasm_bindgen(js_name = "isAttached")]
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Get the attached container associated with this.
    ///
    /// Returns an attached `Container` that equals to this or created by this, otherwise `undefined`.
    #[wasm_bindgen(js_name = "getAttached")]
    pub fn get_attached(&self) -> JsLoroListOrUndefined {
        if self.is_attached() {
            let value: JsValue = self.clone().into();
            return value.into();
        }

        if let Some(h) = self.handler.get_attached() {
            handler_to_js_value(Handler::MovableList(h), self.doc.clone()).into()
        } else {
            JsValue::UNDEFINED.into()
        }
    }

    /// Get the cursor of the container.
    #[wasm_bindgen(skip_typescript)]
    pub fn getCursor(&self, pos: usize, side: JsSide) -> Option<Cursor> {
        let mut side_value = Side::Middle;
        if side.is_truthy() {
            let num = side.as_f64().expect("Side must be -1 | 0 | 1");
            side_value = Side::from_i32(num as i32).expect("Side must be -1 | 0 | 1");
        }
        self.handler
            .get_cursor(pos, side_value)
            .map(|pos| Cursor { pos })
    }

    /// Move the element from `from` to `to`.
    ///
    /// The new position of the element will be `to`.
    /// Move the element from `from` to `to`.
    ///
    /// The new position of the element will be `to`. This method is optimized to prevent redundant
    /// operations that might occur with a naive remove and insert approach. Specifically, it avoids
    /// creating surplus values in the list, unlike a delete followed by an insert, which can lead to
    /// additional values in cases of concurrent edits. This ensures more efficient and accurate
    /// operations in a MovableList.
    #[wasm_bindgen(js_name = "move")]
    pub fn mov(&self, from: usize, to: usize) -> JsResult<()> {
        self.handler.mov(from, to)?;
        Ok(())
    }

    /// Set the value at the given position.
    ///
    /// It's different from `delete` + `insert` that it will replace the value at the position.
    ///
    /// For example, if you have a list `[1, 2, 3]`, and you call `set(1, 100)`, the list will be `[1, 100, 3]`.
    /// If concurrently someone call `set(1, 200)`, the list will be `[1, 200, 3]` or `[1, 100, 3]`.
    ///
    /// But if you use `delete` + `insert` to simulate the set operation, they may create redundant operations
    /// and the final result will be `[1, 100, 200, 3]` or `[1, 200, 100, 3]`.
    #[wasm_bindgen(skip_typescript)]
    pub fn set(&self, pos: usize, value: JsLoroValue) -> JsResult<()> {
        let v: JsValue = value.into();
        self.handler.set(pos, v)?;
        Ok(())
    }

    /// Set the container at the given position.
    #[wasm_bindgen(skip_typescript)]
    pub fn setContainer(&self, pos: usize, child: JsContainer) -> JsResult<JsContainer> {
        let child = js_to_container(child)?;
        let c = self.handler.set_container(pos, child.to_handler())?;
        Ok(handler_to_js_value(c, self.doc.clone()).into())
    }

    /// Push a value to the end of the list.
    #[wasm_bindgen(skip_typescript)]
    pub fn push(&self, value: JsLoroValue) -> JsResult<()> {
        let v: JsValue = value.into();
        self.handler.push(v.into())?;
        Ok(())
    }

    /// Pop a value from the end of the list.
    pub fn pop(&self) -> JsResult<Option<JsLoroValue>> {
        let v = self.handler.pop()?;
        Ok(v.map(|v| {
            let v: JsValue = v.into();
            v.into()
        }))
    }

    /// Delete all elements in the list.
    pub fn clear(&self) -> JsResult<()> {
        self.handler.clear()?;
        Ok(())
    }

    /// Get the creator of the list item at the given position.
    pub fn getCreatorAt(&self, pos: usize) -> Option<JsStrPeerID> {
        self.handler.get_creator_at(pos).map(peer_id_to_js)
    }

    /// Get the last mover of the list item at the given position.
    pub fn getLastMoverAt(&self, pos: usize) -> Option<JsStrPeerID> {
        self.handler.get_last_mover_at(pos).map(peer_id_to_js)
    }

    /// Get the last editor of the list item at the given position.
    pub fn getLastEditorAt(&self, pos: usize) -> Option<JsStrPeerID> {
        self.handler.get_last_editor_at(pos).map(peer_id_to_js)
    }
}

/// The handler of a tree(forest) container.
///
/// Learn more at https://loro.dev/docs/tutorial/tree
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroTree {
    handler: TreeHandler,
    doc: Option<Arc<LoroDocInner>>,
}

extern crate alloc;
/// The handler of a tree node.
#[allow(missing_docs)]
#[derive(TryFromJsValue, Clone)]
#[wasm_bindgen]
pub struct LoroTreeNode {
    id: TreeID,
    tree: TreeHandler,
    doc: Option<Arc<LoroDocInner>>,
}

fn parse_js_parent(parent: &JsParentTreeID) -> JsResult<Option<TreeID>> {
    let js_value: JsValue = parent.into();
    let parent: Option<TreeID> = if js_value.is_undefined() {
        None
    } else {
        Some(TreeID::try_from(js_value)?)
    };
    Ok(parent)
}

fn parse_js_tree_node(parent: &JsTreeNodeOrUndefined) -> JsResult<Option<LoroTreeNode>> {
    let js_value: &JsValue = parent.as_ref();
    let parent: Option<LoroTreeNode> = if js_value.is_undefined() {
        None
    } else {
        Some(LoroTreeNode::try_from(js_value)?)
    };
    Ok(parent)
}

// TODO: avoid converting
fn parse_js_tree_id(target: &JsTreeID) -> JsResult<TreeID> {
    let target: JsValue = target.into();
    let target = TreeID::try_from(target)?;
    Ok(target)
}

#[wasm_bindgen]
impl LoroTreeNode {
    fn from_tree(id: TreeID, tree: TreeHandler, doc: Option<Arc<LoroDocInner>>) -> Self {
        Self { id, tree, doc }
    }

    /// The TreeID of the node.
    #[wasm_bindgen(getter, js_name = "id")]
    pub fn id(&self) -> JsTreeID {
        let value: JsValue = self.id.into();
        value.into()
    }

    /// Create a new node as the child of the current node and
    /// return an instance of `LoroTreeNode`.
    ///
    /// If the index is not provided, the new node will be appended to the end.
    ///
    /// @example
    /// ```typescript
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// let doc = new LoroDoc();
    /// let tree = doc.getTree("tree");
    /// let root = tree.createNode();
    /// let node = root.createNode();
    /// let node2 = root.createNode(0);
    /// //    root
    /// //    /  \
    /// // node2 node
    /// ```
    #[wasm_bindgen(js_name = "createNode", skip_typescript)]
    pub fn create_node(&self, index: Option<usize>) -> JsResult<LoroTreeNode> {
        let id = if let Some(index) = index {
            self.tree.create_at(TreeParentId::Node(self.id), index)?
        } else {
            self.tree.create(TreeParentId::Node(self.id))?
        };
        let node = LoroTreeNode::from_tree(id, self.tree.clone(), self.doc.clone());
        Ok(node)
    }

    /// Move this tree node to be a child of the parent.
    /// If the parent is undefined, this node will be a root node.
    ///
    /// If the index is not provided, the node will be appended to the end.
    ///
    /// It's not allowed that the target is an ancestor of the parent.
    ///
    /// @example
    /// ```ts
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// const node2 = node.createNode();
    /// node2.move(undefined, 0);
    /// // node2   root
    /// //          |
    /// //         node
    ///
    /// ```
    #[wasm_bindgen(js_name = "move")]
    pub fn mov(&self, parent: &JsTreeNodeOrUndefined, index: Option<usize>) -> JsResult<()> {
        let parent: Option<LoroTreeNode> = parse_js_tree_node(parent)?;
        if let Some(index) = index {
            self.tree
                .move_to(self.id, parent.map(|x| x.id).into(), index)?
        } else {
            self.tree.mov(self.id, parent.map(|x| x.id).into())?;
        }

        Ok(())
    }

    /// Move the tree node to be after the target node.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// const node2 = root.createNode();
    /// node2.moveAfter(node);
    /// // root
    /// //  /  \
    /// // node node2
    /// ```
    #[wasm_bindgen(js_name = "moveAfter")]
    pub fn mov_after(&self, target: &LoroTreeNode) -> JsResult<()> {
        self.tree.mov_after(self.id, target.id)?;
        Ok(())
    }

    /// Move the tree node to be before the target node.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// const node2 = root.createNode();
    /// node2.moveBefore(node);
    /// //   root
    /// //  /    \
    /// // node2 node
    /// ```
    #[wasm_bindgen(js_name = "moveBefore")]
    pub fn mov_before(&self, target: &LoroTreeNode) -> JsResult<()> {
        self.tree.mov_before(self.id, target.id)?;
        Ok(())
    }

    /// Get the index of the node in the parent's children.
    #[wasm_bindgen]
    pub fn index(&self) -> JsResult<Option<usize>> {
        let index = self.tree.get_index_by_tree_id(&self.id);
        Ok(index)
    }

    /// Get the `Fractional Index` of the node.
    ///
    /// Note: the tree container must be attached to the document.
    #[wasm_bindgen(js_name = "fractionalIndex")]
    pub fn fractional_index(&self) -> JsResult<JsPositionOrUndefined> {
        if self.tree.is_attached() {
            let pos = self.tree.get_position_by_tree_id(&self.id);
            let ans = if let Some(pos) = pos.map(|x| x.to_string()) {
                JsValue::from_str(&pos).into()
            } else {
                JsValue::UNDEFINED.into()
            };
            Ok(ans)
        } else {
            Err(JsValue::from_str("Tree is detached"))
        }
    }

    /// Get the associated metadata map container of a tree node.
    #[wasm_bindgen(getter, skip_typescript)]
    pub fn data(&self) -> JsResult<LoroMap> {
        let data = self.tree.get_meta(self.id)?;
        let map = LoroMap {
            handler: data,
            doc: self.doc.clone(),
        };
        Ok(map)
    }

    #[wasm_bindgen(js_name = "toJSON", skip_typescript)]
    pub fn get_deep_value(&self) -> JsResult<JsValue> {
        let value = self
            .tree
            .get_all_hierarchy_nodes_under(TreeParentId::Node(self.id));
        let node = TreeNodeWithChildren {
            id: self.id,
            parent: self.tree.get_node_parent(&self.id).unwrap(),
            fractional_index: self.tree.get_position_by_tree_id(&self.id).unwrap(),
            index: self.tree.get_index_by_tree_id(&self.id).unwrap(),
            children: value,
        };
        LoroTree {
            handler: self.tree.clone(),
            doc: self.doc.clone(),
        }
        .tree_node_to_js_obj(node, true)
    }

    /// Get the parent node of this node.
    ///
    /// - The parent of the root node is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    #[wasm_bindgen]
    pub fn parent(&self) -> JsResult<Option<LoroTreeNode>> {
        let parent = self
            .tree
            .get_node_parent(&self.id)
            .ok_or(JsValue::from_str(&format!("TreeID({}) not found", self.id)))?;
        let ans = parent
            .tree_id()
            .map(|p| LoroTreeNode::from_tree(p, self.tree.clone(), self.doc.clone()));
        Ok(ans)
    }

    /// Get the children of this node.
    ///
    /// The objects returned are new js objects each time because they need to cross
    /// the WASM boundary.
    #[wasm_bindgen(skip_typescript)]
    pub fn children(&self) -> JsValue {
        let Some(children) = self.tree.children(&TreeParentId::Node(self.id)) else {
            return JsValue::undefined();
        };
        let children = children.into_iter().map(|c| {
            let node = LoroTreeNode::from_tree(c, self.tree.clone(), self.doc.clone());
            JsValue::from(node)
        });
        Array::from_iter(children).into()
    }

    /// Check if the node is deleted.
    #[wasm_bindgen(js_name = "isDeleted")]
    pub fn is_deleted(&self) -> JsResult<bool> {
        let ans = self.tree.is_node_deleted(&self.id)?;
        Ok(ans)
    }

    /// Get the last mover of this node.
    pub fn getLastMoveId(&self) -> Option<JsID> {
        self.tree
            .get_last_move_id(&self.id)
            .map(|x| id_to_js(&x).into())
    }

    /// Get the creation id of this node.
    pub fn creationId(&self) -> JsID {
        id_to_js(&self.id.id()).into()
    }

    /// Get the creator of this node.
    pub fn creator(&self) -> JsStrPeerID {
        peer_id_to_js(self.id.peer)
    }
}

#[wasm_bindgen]
impl LoroTree {
    /// Create a new detached LoroTree (not attached to any LoroDoc).
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            handler: TreeHandler::new_detached(),
            doc: None,
        }
    }

    /// "Tree"
    pub fn kind(&self) -> JsTreeStr {
        JsValue::from_str("Tree").into()
    }

    /// Create a new tree node as the child of parent and return a `LoroTreeNode` instance.
    /// If the parent is undefined, the tree node will be a root node.
    ///
    /// If the index is not provided, the new node will be appended to the end.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = tree.createNode(undefined, 0);
    ///
    /// //  undefined
    /// //    /   \
    /// // node  root
    /// ```
    #[wasm_bindgen(js_name = "createNode", skip_typescript)]
    pub fn create_node(
        &mut self,
        parent: &JsParentTreeID,
        index: Option<usize>,
    ) -> JsResult<LoroTreeNode> {
        let parent: Option<TreeID> = parse_js_parent(parent)?;
        let id = if let Some(index) = index {
            self.handler.create_at(parent.into(), index)?
        } else {
            self.handler.create(parent.into())?
        };
        let node = LoroTreeNode::from_tree(id, self.handler.clone(), self.doc.clone());
        Ok(node)
    }

    /// Move the target tree node to be a child of the parent.
    /// It's not allowed that the target is an ancestor of the parent
    /// or the target and the parent are the same node.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// const node2 = node.createNode();
    /// tree.move(node2.id, root.id);
    /// // Error will be thrown if move operation creates a cycle
    /// // tree.move(root.id, node.id);
    /// ```
    #[wasm_bindgen(js_name = "move")]
    pub fn mov(
        &mut self,
        target: &JsTreeID,
        parent: &JsParentTreeID,
        index: Option<usize>,
    ) -> JsResult<()> {
        let target = parse_js_tree_id(target)?;
        let parent = parse_js_parent(parent)?;

        if let Some(index) = index {
            self.handler.move_to(target, parent.into(), index)?
        } else {
            self.handler.mov(target, parent.into())?
        };

        Ok(())
    }

    /// Delete a tree node from the forest.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// tree.delete(node.id);
    /// ```
    pub fn delete(&mut self, target: &JsTreeID) -> JsResult<()> {
        let target = parse_js_tree_id(target)?;
        self.handler.delete(target)?;
        Ok(())
    }

    /// Get LoroTreeNode by the TreeID.
    #[wasm_bindgen(js_name = "getNodeByID", skip_typescript)]
    pub fn get_node_by_id(&self, target: &JsTreeID) -> Option<LoroTreeNode> {
        let target: JsValue = target.into();
        let target = TreeID::try_from(target).ok()?;
        if self.handler.is_node_unexist(&target) {
            return None;
        }
        Some(LoroTreeNode::from_tree(
            target,
            self.handler.clone(),
            self.doc.clone(),
        ))
    }

    /// Get the id of the container.
    #[wasm_bindgen(js_name = "id", getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Return `true` if the tree contains the TreeID, include deleted node.
    #[wasm_bindgen(js_name = "has")]
    pub fn contains(&self, target: &JsTreeID) -> bool {
        let target: JsValue = target.into();
        self.handler.contains(target.try_into().unwrap())
    }

    /// Return `None` if the node is not exist, otherwise return `Some(true)` if the node is deleted.
    #[wasm_bindgen(js_name = "isNodeDeleted")]
    pub fn is_node_deleted(&self, target: &JsTreeID) -> JsResult<bool> {
        let target: JsValue = target.into();
        let target = TreeID::try_from(target).unwrap();
        let ans = self.handler.is_node_deleted(&target)?;
        Ok(ans)
    }

    /// Get the hierarchy array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use `toJson()`.
    ///
    // TODO: perf
    #[wasm_bindgen(js_name = "toArray", skip_typescript)]
    pub fn to_array(&self) -> JsResult<Array> {
        let value = self
            .handler
            .get_all_hierarchy_nodes_under(TreeParentId::Root);
        self.get_node_with_children(value, false)
    }

    /// Get the flat array of the forest. If `with_deleted` is true, the deleted nodes will be included.
    #[wasm_bindgen(js_name = "getNodes", skip_typescript)]
    pub fn get_nodes(&self, options: JsGetNodesProp) -> JsResult<Array> {
        let with_deleted = if options.is_undefined() {
            false
        } else {
            Reflect::get(&options.into(), &JsValue::from_str("withDeleted"))?
                .as_bool()
                .unwrap_or(false)
        };
        let nodes = Array::new();
        for v in self.handler.get_nodes_under(TreeParentId::Root) {
            let node = LoroTreeNode::from_tree(v.id, self.handler.clone(), self.doc.clone());
            nodes.push(&node.into());
        }
        if with_deleted {
            for v in self.handler.get_nodes_under(TreeParentId::Deleted) {
                let node = LoroTreeNode::from_tree(v.id, self.handler.clone(), self.doc.clone());
                nodes.push(&node.into());
            }
        }
        Ok(nodes)
    }

    fn get_node_with_children(
        &self,
        value: Vec<TreeNodeWithChildren>,
        resolve_meta: bool,
    ) -> JsResult<Array> {
        let ans = Array::new();
        for v in value {
            ans.push(&self.tree_node_to_js_obj(v, resolve_meta)?);
        }
        Ok(ans)
    }

    fn tree_node_to_js_obj(
        &self,
        v: TreeNodeWithChildren,
        resolve_meta: bool,
    ) -> JsResult<JsValue> {
        let id: JsValue = v.id.into();
        let id: JsTreeID = id.into();
        let parent = v.parent.tree_id();
        let parent = parent
            .map(|x| JsValue::from_str(&x.to_string()))
            .unwrap_or(JsValue::undefined());
        let index = v.index;
        let position = v.fractional_index.to_string();
        let map: LoroMap = self.get_node_by_id(&id).unwrap().data()?;
        let obj = Object::new();
        js_sys::Reflect::set(&obj, &"id".into(), &id)?;
        js_sys::Reflect::set(&obj, &"parent".into(), &parent)?;
        js_sys::Reflect::set(&obj, &"index".into(), &JsValue::from(index))?;
        js_sys::Reflect::set(
            &obj,
            &"fractionalIndex".into(),
            &JsValue::from_str(&position),
        )?;
        if resolve_meta {
            js_sys::Reflect::set(&obj, &"meta".into(), &map.to_json())?;
        } else {
            js_sys::Reflect::set(&obj, &"meta".into(), &map.into())?;
        }
        let children = self.get_node_with_children(v.children, resolve_meta)?;
        js_sys::Reflect::set(&obj, &"children".into(), &children)?;
        Ok(obj.into())
    }

    /// Get the hierarchy array with metadata of the forest.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// root.data.set("color", "red");
    /// // [ { id: '0@F2462C4159C4C8D1', parent: null, meta: { color: 'red' }, children: [] } ]
    /// console.log(tree.toJSON());
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsValue {
        self.handler.get_deep_value().into()
    }

    /// Get all tree nodes of the forest, including deleted nodes.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// const node2 = node.createNode();
    /// console.log(tree.nodes());
    /// ```
    pub fn nodes(&mut self) -> Vec<LoroTreeNode> {
        self.handler
            .nodes()
            .into_iter()
            .map(|n| LoroTreeNode::from_tree(n, self.handler.clone(), self.doc.clone()))
            .collect()
    }

    /// Get the root nodes of the forest.
    pub fn roots(&self) -> Vec<LoroTreeNode> {
        self.handler
            .roots()
            .into_iter()
            .map(|n| LoroTreeNode::from_tree(n, self.handler.clone(), self.doc.clone()))
            .collect()
    }

    /// Subscribe to the changes of the tree.
    ///
    /// Returns a subscription callback, which can be used to unsubscribe.
    ///
    /// Trees have three types of events: `create`, `delete`, and `move`.
    /// - `create`: Creates a new node with its `target` TreeID. If `parent` is undefined,
    ///             a root node is created; otherwise, a child node of `parent` is created.
    ///             If the node being created was previously deleted and has archived child nodes,
    ///             create events for these child nodes will also be received.
    /// - `delete`: Deletes the target node. The structure and state of the target node and
    ///             its child nodes are archived, and delete events for the child nodes will not be received.
    /// - `move`:   Moves the target node. If `parent` is undefined, the target node becomes a root node;
    ///             otherwise, it becomes a child node of `parent`.
    ///
    /// If a tree container is subscribed, the event of metadata changes will also be received as a MapDiff.
    /// And event's `path` will end with `TreeID`.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.export(mode)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// tree.subscribe((event)=>{
    ///     // event.type: "create" | "delete" | "move"
    /// });
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// doc.commit();
    /// ```
    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<JsValue> {
        let observer = observer::Observer::new(f);
        let doc = self
            .doc
            .clone()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = doc.clone();
        let ans = doc.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );
        Ok(subscription_to_js_function_callback(ans))
    }

    /// Get the parent container of the tree container.
    ///
    /// - The parent container of the root tree is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = HandlerTrait::parent(&self.handler) {
            handler_to_js_value(p, self.doc.clone()).into()
        } else {
            JsContainerOrUndefined::from(JsValue::UNDEFINED)
        }
    }

    /// Whether the container is attached to a document.
    ///
    /// If it's detached, the operations on the container will not be persisted.
    #[wasm_bindgen(js_name = "isAttached")]
    pub fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    /// Get the attached container associated with this.
    ///
    /// Returns an attached `Container` that equals to this or created by this, otherwise `undefined`.
    #[wasm_bindgen(js_name = "getAttached")]
    pub fn get_attached(&self) -> JsLoroTreeOrUndefined {
        if self.is_attached() {
            let value: JsValue = self.clone().into();
            return value.into();
        }

        if let Some(h) = self.handler.get_attached() {
            handler_to_js_value(Handler::Tree(h), self.doc.clone()).into()
        } else {
            JsValue::UNDEFINED.into()
        }
    }

    /// Set whether to generate a fractional index for moving and creating.
    ///
    /// A fractional index can be used to determine the position of tree nodes among their siblings.
    ///
    /// The jitter is used to avoid conflicts when multiple users are creating a node at the same position.
    /// A value of 0 is the default, which means no jitter; any value larger than 0 will enable jitter.
    ///
    /// Generally speaking, higher jitter value will increase the size of the operation
    /// [Read more about it](https://www.loro.dev/blog/movable-tree#implementation-and-encoding-size)
    #[wasm_bindgen(js_name = "enableFractionalIndex")]
    pub fn enable_fractional_index(&self, jitter: u8) {
        self.handler.enable_fractional_index(jitter);
    }

    /// Disable the fractional index generation for Tree Position when
    /// you don't need the Tree's siblings to be sorted. The fractional index will always be set to default.
    #[wasm_bindgen(js_name = "disableFractionalIndex")]
    pub fn disable_fractional_index(&self) {
        self.handler.disable_fractional_index();
    }

    /// Whether the tree enables the fractional index generation.
    #[wasm_bindgen(js_name = "isFractionalIndexEnabled")]
    pub fn is_fractional_index_enabled(&self) -> bool {
        self.handler.is_fractional_index_enabled()
    }
}

impl Default for LoroTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Cursor is a stable position representation in the doc.
/// When expressing the position of a cursor, using "index" can be unstable
/// because the cursor's position may change due to other deletions and insertions,
/// requiring updates with each edit. To stably represent a position or range within
/// a list structure, we can utilize the ID of each item/character on List CRDT or
/// Text CRDT for expression.
///
/// Loro optimizes State metadata by not storing the IDs of deleted elements. This
/// approach complicates tracking cursors since they rely on these IDs. The solution
/// recalculates position by replaying relevant history to update cursors
/// accurately. To minimize the performance impact of history replay, the system
/// updates cursor info to reference only the IDs of currently present elements,
/// thereby reducing the need for replay.
///
/// @example
/// ```ts
///
/// const doc = new LoroDoc();
/// const text = doc.getText("text");
/// text.insert(0, "123");
/// const pos0 = text.getCursor(0, 0);
/// {
///   const ans = doc.getCursorPos(pos0!);
///   expect(ans.offset).toBe(0);
/// }
/// text.insert(0, "1");
/// {
///   const ans = doc.getCursorPos(pos0!);
///   expect(ans.offset).toBe(1);
/// }
/// ```
#[derive(Clone)]
#[wasm_bindgen]
pub struct Cursor {
    pos: cursor::Cursor,
}

#[wasm_bindgen]
impl Cursor {
    /// Get the id of the given container.
    pub fn containerId(&self) -> JsContainerID {
        let js_value: JsValue = self.pos.container.to_string().into();
        JsContainerID::from(js_value)
    }

    /// Get the ID that represents the position.
    ///
    /// It can be undefined if it's not bind into a specific ID.
    pub fn pos(&self) -> Option<JsID> {
        match self.pos.id {
            Some(id) => {
                let value: JsValue = id_to_js(&id);
                Some(value.into())
            }
            None => None,
        }
    }

    /// Get which side of the character/list item the cursor is on.
    pub fn side(&self) -> JsSide {
        JsValue::from(match self.pos.side {
            cursor::Side::Left => -1,
            cursor::Side::Middle => 0,
            cursor::Side::Right => 1,
        })
        .into()
    }

    /// Encode the cursor into a Uint8Array.
    pub fn encode(&self) -> Vec<u8> {
        self.pos.encode()
    }

    /// Decode the cursor from a Uint8Array.
    pub fn decode(data: &[u8]) -> JsResult<Cursor> {
        let pos = cursor::Cursor::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Cursor { pos })
    }

    /// "Cursor"
    pub fn kind(&self) -> JsValue {
        JsValue::from_str("Cursor")
    }
}

fn loro_value_to_js_value_or_container(
    value: ValueOrHandler,
    doc: Option<Arc<LoroDocInner>>,
) -> JsValue {
    match value {
        ValueOrHandler::Value(v) => {
            let value: JsValue = v.into();
            value
        }
        ValueOrHandler::Handler(c) => {
            let handler: JsValue = handler_to_js_value(c, doc.clone());
            handler
        }
    }
}

/// `UndoManager` is responsible for handling undo and redo operations.
///
/// By default, the maxUndoSteps is set to 100, mergeInterval is set to 1000 ms.
///
/// Each commit made by the current peer is recorded as an undo step in the `UndoManager`.
/// Undo steps can be merged if they occur within a specified merge interval.
///
/// Note that undo operations are local and cannot revert changes made by other peers.
/// To undo changes made by other peers, consider using the time travel feature.
///
/// Once the `peerId` is bound to the `UndoManager` in the document, it cannot be changed.
/// Otherwise, the `UndoManager` may not function correctly.
#[wasm_bindgen]
#[derive(Debug)]
pub struct UndoManager {
    undo: InnerUndoManager,
    doc: Arc<LoroDocInner>,
}

#[wasm_bindgen]
impl UndoManager {
    /// `UndoManager` is responsible for handling undo and redo operations.
    ///
    /// PeerID cannot be changed during the lifetime of the UndoManager.
    ///
    /// Note that undo operations are local and cannot revert changes made by other peers.
    /// To undo changes made by other peers, consider using the time travel feature.
    ///
    /// Each commit made by the current peer is recorded as an undo step in the `UndoManager`.
    /// Undo steps can be merged if they occur within a specified merge interval.
    ///
    /// ## Config
    ///
    /// - `mergeInterval`: Optional. The interval in milliseconds within which undo steps can be merged. Default is 1000 ms.
    /// - `maxUndoSteps`: Optional. The maximum number of undo steps to retain. Default is 100.
    /// - `excludeOriginPrefixes`: Optional. An array of string prefixes. Events with origins matching these prefixes will be excluded from undo steps.
    /// - `onPush`: Optional. A callback function that is called when an undo/redo step is pushed.
    ///    The function can return a meta data value that will be attached to the given stack item.
    /// - `onPop`: Optional. A callback function that is called when an undo/redo step is popped.
    ///    The function will have a meta data value that was attached to the given stack item when
    ///   `onPush` was called.
    #[wasm_bindgen(constructor)]
    pub fn new(doc: &LoroDoc, config: JsUndoConfig) -> Self {
        let max_undo_steps = Reflect::get(&config, &JsValue::from_str("maxUndoSteps"))
            .unwrap_or(JsValue::from_f64(100.0))
            .as_f64()
            .unwrap_or(100.0) as usize;
        let merge_interval = Reflect::get(&config, &JsValue::from_str("mergeInterval"))
            .unwrap_or(JsValue::from_f64(1000.0))
            .as_f64()
            .unwrap_or(1000.0) as i64;
        let exclude_origin_prefixes =
            Reflect::get(&config, &JsValue::from_str("excludeOriginPrefixes"))
                .ok()
                .and_then(|val| val.dyn_into::<js_sys::Array>().ok())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|val| val.as_string())
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
        let on_push = Reflect::get(&config, &JsValue::from_str("onPush")).ok();
        let on_pop = Reflect::get(&config, &JsValue::from_str("onPop")).ok();

        let mut undo = InnerUndoManager::new(&doc.0);
        undo.set_max_undo_steps(max_undo_steps);
        undo.set_merge_interval(merge_interval);
        for prefix in exclude_origin_prefixes {
            undo.add_exclude_origin_prefix(&prefix);
        }

        let mut ans = UndoManager {
            undo,
            doc: doc.0.clone(),
        };

        if let Some(on_push) = on_push {
            ans.setOnPush(on_push);
        }
        if let Some(on_pop) = on_pop {
            ans.setOnPop(on_pop);
        }
        ans
    }

    /// Undo the last operation.
    pub fn undo(&mut self) -> JsResult<bool> {
        let executed = self.undo.undo(&self.doc)?;
        Ok(executed)
    }

    /// Redo the last undone operation.
    pub fn redo(&mut self) -> JsResult<bool> {
        let executed = self.undo.redo(&self.doc)?;
        Ok(executed)
    }

    /// Can undo the last operation.
    pub fn canUndo(&self) -> bool {
        self.undo.can_undo()
    }

    /// Can redo the last operation.
    pub fn canRedo(&self) -> bool {
        self.undo.can_redo()
    }

    /// The number of max undo steps.
    /// If the number of undo steps exceeds this number, the oldest undo step will be removed.
    pub fn setMaxUndoSteps(&mut self, steps: usize) {
        self.undo.set_max_undo_steps(steps);
    }

    /// Set the merge interval (in ms).
    /// If the interval is set to 0, the undo steps will not be merged.
    /// Otherwise, the undo steps will be merged if the interval between the two steps is less than the given interval.
    pub fn setMergeInterval(&mut self, interval: f64) {
        self.undo.set_merge_interval(interval as i64);
    }

    /// If a local event's origin matches the given prefix, it will not be recorded in the
    /// undo stack.
    pub fn addExcludeOriginPrefix(&mut self, prefix: String) {
        self.undo.add_exclude_origin_prefix(&prefix)
    }

    /// Check if the undo manager is bound to the given document.
    pub fn checkBinding(&self, doc: &LoroDoc) -> bool {
        Arc::ptr_eq(&self.doc, &doc.0)
    }

    /// Set the on push event listener.
    ///
    /// Every time an undo step or redo step is pushed, the on push event listener will be called.
    #[wasm_bindgen(skip_typescript)]
    pub fn setOnPush(&mut self, on_push: JsValue) {
        let on_push = on_push.dyn_into::<js_sys::Function>().ok();
        if let Some(on_push) = on_push {
            let on_push = observer::Observer::new(on_push);
            self.undo.set_on_push(Some(Box::new(move |kind, span| {
                let is_undo = JsValue::from_bool(matches!(kind, UndoOrRedo::Undo));
                let counter_range = js_sys::Object::new();
                js_sys::Reflect::set(
                    &counter_range,
                    &JsValue::from_str("start"),
                    &JsValue::from_f64(span.start as f64),
                )
                .unwrap();
                js_sys::Reflect::set(
                    &counter_range,
                    &JsValue::from_str("end"),
                    &JsValue::from_f64(span.end as f64),
                )
                .unwrap();

                let mut undo_item_meta = UndoItemMeta::new();
                match on_push.call2(&is_undo, &counter_range) {
                    Ok(v) => {
                        if let Ok(obj) = v.dyn_into::<js_sys::Object>() {
                            if let Ok(value) =
                                js_sys::Reflect::get(&obj, &JsValue::from_str("value"))
                            {
                                let value: LoroValue = value.into();
                                undo_item_meta.value = value;
                            }
                            if let Ok(cursors) =
                                js_sys::Reflect::get(&obj, &JsValue::from_str("cursors"))
                            {
                                let cursors: js_sys::Array = cursors.into();
                                for cursor in cursors.iter() {
                                    let cursor = js_to_cursor(cursor).unwrap_throw();
                                    undo_item_meta.add_cursor(&cursor.pos);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        throw_error_after_micro_task(e);
                    }
                }

                undo_item_meta
            })));
        } else {
            self.undo.set_on_push(None);
        }
    }

    /// Set the on pop event listener.
    ///
    /// Every time an undo step or redo step is popped, the on pop event listener will be called.
    #[wasm_bindgen(skip_typescript)]
    pub fn setOnPop(&mut self, on_pop: JsValue) {
        let on_pop = on_pop.dyn_into::<js_sys::Function>().ok();
        if let Some(on_pop) = on_pop {
            let on_pop = observer::Observer::new(on_pop);
            self.undo
                .set_on_pop(Some(Box::new(move |kind, span, value| {
                    let is_undo = JsValue::from_bool(matches!(kind, UndoOrRedo::Undo));
                    let meta = js_sys::Object::new();
                    js_sys::Reflect::set(&meta, &JsValue::from_str("value"), &value.value.into())
                        .unwrap();
                    let cursors_array = js_sys::Array::new();
                    for cursor in value.cursors {
                        let c = Cursor { pos: cursor.cursor };
                        cursors_array.push(&c.into());
                    }
                    js_sys::Reflect::set(&meta, &JsValue::from_str("cursors"), &cursors_array)
                        .unwrap();
                    let counter_range = js_sys::Object::new();
                    js_sys::Reflect::set(
                        &counter_range,
                        &JsValue::from_str("start"),
                        &JsValue::from_f64(span.start as f64),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &counter_range,
                        &JsValue::from_str("end"),
                        &JsValue::from_f64(span.end as f64),
                    )
                    .unwrap();
                    match on_pop.call3(&is_undo, &meta.into(), &counter_range) {
                        Ok(_) => {}
                        Err(e) => {
                            throw_error_after_micro_task(e);
                        }
                    }
                })));
        } else {
            self.undo.set_on_pop(None);
        }
    }

    pub fn clear(&self) {
        self.undo.clear();
    }
}

/// Use this function to throw an error after the micro task.
///
/// We should avoid panic or use js_throw directly inside a event listener as it might
/// break the internal invariants.
fn throw_error_after_micro_task(error: JsValue) {
    let drop_handler = Rc::new(RefCell::new(None));
    let drop_handler_clone = drop_handler.clone();
    let closure = Closure::once(Box::new(move |_| {
        drop(drop_handler_clone);
        throw_val(error);
    }));
    let promise = Promise::resolve(&JsValue::NULL);
    let _ = promise.then(&closure);
    drop_handler.borrow_mut().replace(closure);
}

/// [VersionVector](https://en.wikipedia.org/wiki/Version_vector)
/// is a map from [PeerID] to [Counter]. Its a right-open interval.
///
/// i.e. a [VersionVector] of `{A: 1, B: 2}` means that A has 1 atomic op and B has 2 atomic ops,
/// thus ID of `{client: A, counter: 1}` is out of the range.
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct VersionVector(pub(crate) InternalVersionVector);

#[wasm_bindgen]
impl VersionVector {
    /// Create a new version vector.
    #[wasm_bindgen(constructor)]
    pub fn new(value: JsIntoVersionVector) -> JsResult<VersionVector> {
        let value: JsValue = value.into();
        if value.is_null() || value.is_undefined() {
            return Ok(Self::default());
        }

        let is_bytes = value.is_instance_of::<js_sys::Uint8Array>();
        if is_bytes {
            let bytes = js_sys::Uint8Array::from(value.clone());
            let bytes = bytes.to_vec();
            return VersionVector::decode(&bytes);
        }

        VersionVector::from_json(JsVersionVectorMap::from(value))
    }

    /// Create a new version vector from a Map.
    #[wasm_bindgen(js_name = "parseJSON", method)]
    pub fn from_json(version: JsVersionVectorMap) -> JsResult<VersionVector> {
        let map: JsValue = version.into();
        let map: js_sys::Map = map.into();
        let mut vv = InternalVersionVector::new();
        for pair in map.entries() {
            let pair = pair.unwrap_throw();
            let key = Reflect::get(&pair, &0.into()).unwrap_throw();
            let peer_id = key.as_string().expect_throw("PeerID must be string");
            let value = Reflect::get(&pair, &1.into()).unwrap_throw();
            let counter = value.as_f64().expect_throw("Invalid counter") as Counter;
            vv.insert(
                peer_id
                    .parse()
                    .expect_throw(&format!("{} cannot be parsed as u64", peer_id)),
                counter,
            );
        }

        Ok(Self(vv))
    }

    /// Convert the version vector to a Map
    #[wasm_bindgen(js_name = "toJSON", method)]
    pub fn to_json(&self) -> JsVersionVectorMap {
        let vv = &self.0;
        let map = js_sys::Map::new();
        for (k, v) in vv.iter() {
            let k = k.to_string().into();
            let v = JsValue::from(*v);
            map.set(&k, &v);
        }

        let value: JsValue = map.into();
        JsVersionVectorMap::from(value)
    }

    /// Encode the version vector into a Uint8Array.
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    /// Decode the version vector from a Uint8Array.
    pub fn decode(bytes: &[u8]) -> JsResult<VersionVector> {
        let vv = InternalVersionVector::decode(bytes)?;
        Ok(Self(vv))
    }

    /// Get the counter of a peer.
    pub fn get(&self, peer_id: JsIntoPeerID) -> JsResult<Option<Counter>> {
        let id = js_peer_to_peer(peer_id.into())?;
        Ok(self.0.get(&id).copied())
    }

    /// Compare the version vector with another version vector.
    ///
    /// If they are concurrent, return undefined.
    pub fn compare(&self, other: &VersionVector) -> Option<i32> {
        self.0.partial_cmp(&other.0).map(|o| match o {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        })
    }

    /// set the exclusive ending point. target id will NOT be included by self
    pub fn setEnd(&mut self, id: JsID) -> JsResult<()> {
        let id = js_id_to_id(id)?;
        self.0.set_end(id);
        Ok(())
    }

    /// set the inclusive ending point. target id will be included
    pub fn setLast(&mut self, id: JsID) -> JsResult<()> {
        let id = js_id_to_id(id)?;
        self.0.set_last(id);
        Ok(())
    }

    pub fn remove(&mut self, peer: JsStrPeerID) -> LoroResult<()> {
        let peer = js_peer_to_peer(peer.into())?;
        self.0.remove(&peer);
        Ok(())
    }

    pub fn length(&self) -> usize {
        self.0.len()
    }
}

const ID_CONVERT_ERROR: &str = "Invalid peer id. It must be a number, a BigInt, or a decimal string that can be parsed to a unsigned 64-bit integer";
fn js_peer_to_peer(value: JsValue) -> JsResult<u64> {
    if value.is_bigint() {
        let bigint = js_sys::BigInt::from(value);
        let v: u64 = bigint
            .try_into()
            .map_err(|_| JsValue::from_str(ID_CONVERT_ERROR))?;
        Ok(v)
    } else if value.is_string() {
        let v: u64 = value
            .as_string()
            .unwrap()
            .parse()
            .expect_throw(ID_CONVERT_ERROR);
        Ok(v)
    } else if let Some(v) = value.as_f64() {
        Ok(v as u64)
    } else {
        Err(JsValue::from_str(ID_CONVERT_ERROR))
    }
}

enum Container {
    Text(LoroText),
    Map(LoroMap),
    List(LoroList),
    Tree(LoroTree),
    MovableList(LoroMovableList),
}

impl Container {
    fn to_handler(&self) -> Handler {
        match self {
            Container::Text(t) => Handler::Text(t.handler.clone()),
            Container::Map(m) => Handler::Map(m.handler.clone()),
            Container::List(l) => Handler::List(l.handler.clone()),
            Container::Tree(t) => Handler::Tree(t.handler.clone()),
            Container::MovableList(l) => Handler::MovableList(l.handler.clone()),
        }
    }
}

/// Decode the metadata of the import blob.
///
/// This method is useful to get the following metadata of the import blob:
///
/// - startVersionVector
/// - endVersionVector
/// - startTimestamp
/// - endTimestamp
/// - mode
/// - changeNum
#[wasm_bindgen(js_name = "decodeImportBlobMeta")]
pub fn decode_import_blob_meta(
    blob: &[u8],
    check_checksum: bool,
) -> JsResult<JsImportBlobMetadata> {
    let meta: ImportBlobMetadata = LoroDocInner::decode_import_blob_meta(blob, check_checksum)?;
    Ok(meta.into())
}

fn js_to_export_mode(js_mode: JsExportMode) -> JsResult<ExportMode<'static>> {
    let js_value: JsValue = js_mode.into();
    let mode = js_sys::Reflect::get(&js_value, &JsValue::from_str("mode"))?
        .as_string()
        .ok_or_else(|| JsError::new("Invalid mode"))?;

    match mode.as_str() {
        "update" => {
            let from = js_sys::Reflect::get(&js_value, &JsValue::from_str("from"))?;
            if from.is_undefined() {
                Ok(ExportMode::all_updates())
            } else {
                let from = js_to_version_vector(from)?;
                // TODO: PERF: avoid this clone
                Ok(ExportMode::updates_owned(from.0.clone()))
            }
        }
        "snapshot" => Ok(ExportMode::Snapshot),
        "shallow-snapshot" => {
            let frontiers: JsValue =
                js_sys::Reflect::get(&js_value, &JsValue::from_str("frontiers"))?;
            let frontiers: Vec<JsID> = js_sys::try_iter(&frontiers)?
                .ok_or_else(|| JsError::new("frontiers is not iterable"))?
                .map(|res| res.map(JsID::from))
                .collect::<Result<_, _>>()?;
            let frontiers = ids_to_frontiers(frontiers)?;
            Ok(ExportMode::shallow_snapshot_owned(frontiers))
        }
        "updates-in-range" => {
            let spans = js_sys::Reflect::get(&js_value, &JsValue::from_str("spans"))?;
            let spans: js_sys::Array = spans.dyn_into()?;
            let mut rust_spans = Vec::new();
            for span in spans.iter() {
                let id = js_sys::Reflect::get(&span, &JsValue::from_str("id"))?;
                let len = js_sys::Reflect::get(&span, &JsValue::from_str("len"))?
                    .as_f64()
                    .ok_or_else(|| JsError::new("Invalid len"))?;
                let id = js_id_to_id(id.into())?;
                rust_spans.push(id.to_span(len as usize));
            }
            Ok(ExportMode::updates_in_range(rust_spans))
        }
        _ => Err(JsError::new("Invalid export mode").into()),
    }
}

fn subscription_to_js_function_callback(sub: Subscription) -> JsValue {
    struct JsSubscription {
        sub: Option<Subscription>,
    }

    impl Drop for JsSubscription {
        fn drop(&mut self) {
            if let Some(sub) = self.sub.take() {
                sub.detach();
            }
        }
    }

    let mut sub: JsSubscription = JsSubscription { sub: Some(sub) };
    let closure = Closure::wrap(Box::new(move || {
        if let Some(sub) = sub.sub.take() {
            sub.unsubscribe()
        }
    }) as Box<dyn FnMut()>);

    closure.into_js_value()
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
/**
* Container types supported by loro.
*
* It is most commonly used to specify the type of sub-container to be created.
* @example
* ```ts
* import { LoroDoc, LoroText } from "loro-crdt";
*
* const doc = new LoroDoc();
* const list = doc.getList("list");
* list.insert(0, 100);
* const text = list.insertContainer(1, new LoroText());
* ```
*/
export type ContainerType = "Text" | "Map" | "List"| "Tree" | "MovableList";

export type PeerID = `${number}`;
/**
* The unique id of each container.
*
* @example
* ```ts
* import { LoroDoc } from "loro-crdt";
*
* const doc = new LoroDoc();
* const list = doc.getList("list");
* const containerId = list.id;
* ```
*/
export type ContainerID =
  | `cid:root-${string}:${ContainerType}`
  | `cid:${number}@${PeerID}:${ContainerType}`;

/**
 * The unique id of each tree node.
 */
export type TreeID = `${number}@${PeerID}`;

interface LoroDoc {
    /**
     * Export updates from the specific version to the current version
     *
     * @deprecated Use `export({mode: "update", from: version})` instead
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const text = doc.getText("text");
     *  text.insert(0, "Hello");
     *  // get all updates of the doc
     *  const updates = doc.exportFrom();
     *  const version = doc.oplogVersion();
     *  text.insert(5, " World");
     *  // get updates from specific version to the latest version
     *  const updates2 = doc.exportFrom(version);
     *  ```
     */
    exportFrom(version?: VersionVector): Uint8Array;
    /**
     *
     *  Get the container corresponding to the container id
     *
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  let text = doc.getText("text");
     *  const textId = text.id;
     *  text = doc.getContainerById(textId);
     *  ```
     */
    getContainerById(id: ContainerID): Container;

    /**
     * Subscribe to updates from local edits.
     *
     * This method allows you to listen for local changes made to the document.
     * It's useful for syncing changes with other instances or saving updates.
     *
     * @param f - A callback function that receives a Uint8Array containing the update data.
     * @returns A function to unsubscribe from the updates.
     *
     * @example
     * ```ts
     * const loro = new Loro();
     * const text = loro.getText("text");
     *
     * const unsubscribe = loro.subscribeLocalUpdates((update) => {
     *   console.log("Local update received:", update);
     *   // You can send this update to other Loro instances
     * });
     *
     * text.insert(0, "Hello");
     * loro.commit();
     *
     * // Later, when you want to stop listening:
     * unsubscribe();
     * ```
     *
     * @example
     * ```ts
     * const loro1 = new Loro();
     * const loro2 = new Loro();
     *
     * // Set up two-way sync
     * loro1.subscribeLocalUpdates((updates) => {
     *   loro2.import(updates);
     * });
     *
     * loro2.subscribeLocalUpdates((updates) => {
     *   loro1.import(updates);
     * });
     *
     * // Now changes in loro1 will be reflected in loro2 and vice versa
     * ```
     */
    subscribeLocalUpdates(f: (bytes: Uint8Array) => void): () => void
}

/**
 * Represents a `Delta` type which is a union of different operations that can be performed.
 *
 * @typeparam T - The data type for the `insert` operation.
 *
 * The `Delta` type can be one of three distinct shapes:
 *
 * 1. Insert Operation:
 *    - `insert`: The item to be inserted, of type T.
 *    - `attributes`: (Optional) A dictionary of attributes, describing styles in richtext
 *
 * 2. Delete Operation:
 *    - `delete`: The number of elements to delete.
 *
 * 3. Retain Operation:
 *    - `retain`: The number of elements to retain.
 *    - `attributes`: (Optional) A dictionary of attributes, describing styles in richtext
 */
export type Delta<T> =
  | {
    insert: T;
    attributes?: { [key in string]: {} };
    retain?: undefined;
    delete?: undefined;
  }
  | {
    delete: number;
    attributes?: undefined;
    retain?: undefined;
    insert?: undefined;
  }
  | {
    retain: number;
    attributes?: { [key in string]: {} };
    delete?: undefined;
    insert?: undefined;
  };

/**
 * The unique id of each operation.
 */
export type OpId = { peer: PeerID, counter: number };

/**
 * Change is a group of continuous operations
 */
export interface Change {
    peer: PeerID,
    counter: number,
    lamport: number,
    length: number,
    /**
     * The timestamp in seconds.
     *
     * [Unix time](https://en.wikipedia.org/wiki/Unix_time)
     * It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
     */
    timestamp: number,
    deps: OpId[],
    message: string | undefined,
}


/**
 * Data types supported by loro
 */
export type Value =
  | ContainerID
  | string
  | number
  | boolean
  | null
  | { [key: string]: Value }
  | Uint8Array
  | Value[];

export type UndoConfig = {
    mergeInterval?: number,
    maxUndoSteps?: number,
    excludeOriginPrefixes?: string[],
    onPush?: (isUndo: boolean, counterRange: { start: number, end: number }) => { value: Value, cursors: Cursor[] },
    onPop?: (isUndo: boolean, value: { value: Value, cursors: Cursor[] }, counterRange: { start: number, end: number }) => void
};
export type Container = LoroList | LoroMap | LoroText | LoroTree | LoroMovableList;

export interface ImportBlobMetadata {
    /**
     * The version vector of the start of the import.
     *
     * Import blob includes all the ops from `partial_start_vv` to `partial_end_vv`.
     * However, it does not constitute a complete version vector, as it only contains counters
     * from peers included within the import blob.
     */
    partialStartVersionVector: VersionVector;
    /**
     * The version vector of the end of the import.
     *
     * Import blob includes all the ops from `partial_start_vv` to `partial_end_vv`.
     * However, it does not constitute a complete version vector, as it only contains counters
     * from peers included within the import blob.
     */
    partialEndVersionVector: VersionVector;

    startFrontiers: OpId[],
    startTimestamp: number;
    endTimestamp: number;
    mode: "outdated-snapshot" | "outdated-update" | "snapshot" | "shallow-snapshot" | "update";
    changeNum: number;
}

interface LoroText {
    /**
     * Get the cursor position at the given pos.
     *
     * When expressing the position of a cursor, using "index" can be unstable
     * because the cursor's position may change due to other deletions and insertions,
     * requiring updates with each edit. To stably represent a position or range within
     * a list structure, we can utilize the ID of each item/character on List CRDT or
     * Text CRDT for expression.
     *
     * Loro optimizes State metadata by not storing the IDs of deleted elements. This
     * approach complicates tracking cursors since they rely on these IDs. The solution
     * recalculates position by replaying relevant history to update cursors
     * accurately. To minimize the performance impact of history replay, the system
     * updates cursor info to reference only the IDs of currently present elements,
     * thereby reducing the need for replay.
     *
     * @example
     * ```ts
     *
     * const doc = new LoroDoc();
     * const text = doc.getText("text");
     * text.insert(0, "123");
     * const pos0 = text.getCursor(0, 0);
     * {
     *   const ans = doc.getCursorPos(pos0!);
     *   expect(ans.offset).toBe(0);
     * }
     * text.insert(0, "1");
     * {
     *   const ans = doc.getCursorPos(pos0!);
     *   expect(ans.offset).toBe(1);
     * }
     * ```
     */
    getCursor(pos: number, side?: Side): Cursor | undefined;
}

interface LoroList {
    /**
     * Get the cursor position at the given pos.
     *
     * When expressing the position of a cursor, using "index" can be unstable
     * because the cursor's position may change due to other deletions and insertions,
     * requiring updates with each edit. To stably represent a position or range within
     * a list structure, we can utilize the ID of each item/character on List CRDT or
     * Text CRDT for expression.
     *
     * Loro optimizes State metadata by not storing the IDs of deleted elements. This
     * approach complicates tracking cursors since they rely on these IDs. The solution
     * recalculates position by replaying relevant history to update cursors
     * accurately. To minimize the performance impact of history replay, the system
     * updates cursor info to reference only the IDs of currently present elements,
     * thereby reducing the need for replay.
     *
     * @example
     * ```ts
     *
     * const doc = new LoroDoc();
     * const text = doc.getList("list");
     * text.insert(0, "1");
     * const pos0 = text.getCursor(0, 0);
     * {
     *   const ans = doc.getCursorPos(pos0!);
     *   expect(ans.offset).toBe(0);
     * }
     * text.insert(0, "1");
     * {
     *   const ans = doc.getCursorPos(pos0!);
     *   expect(ans.offset).toBe(1);
     * }
     * ```
     */
    getCursor(pos: number, side?: Side): Cursor | undefined;
}

export type TreeNodeValue = {
    id: TreeID,
    parent: TreeID | undefined,
    index: number,
    fractionalIndex: string,
    meta: LoroMap,
    children: TreeNodeValue[],
}

export type TreeNodeJSON<T> = Omit<TreeNodeValue, 'meta' | 'children'> & {
    meta: T,
    children: TreeNodeJSON<T>[],
}

interface LoroTree{
    toArray(): TreeNodeValue[];
    getNodes(options?: { withDeleted: boolean = false }): LoroTreeNode[];
}

interface LoroMovableList {
    /**
     * Get the cursor position at the given pos.
     *
     * When expressing the position of a cursor, using "index" can be unstable
     * because the cursor's position may change due to other deletions and insertions,
     * requiring updates with each edit. To stably represent a position or range within
     * a list structure, we can utilize the ID of each item/character on List CRDT or
     * Text CRDT for expression.
     *
     * Loro optimizes State metadata by not storing the IDs of deleted elements. This
     * approach complicates tracking cursors since they rely on these IDs. The solution
     * recalculates position by replaying relevant history to update cursors
     * accurately. To minimize the performance impact of history replay, the system
     * updates cursor info to reference only the IDs of currently present elements,
     * thereby reducing the need for replay.
     *
     * @example
     * ```ts
     *
     * const doc = new LoroDoc();
     * const text = doc.getMovableList("text");
     * text.insert(0, "1");
     * const pos0 = text.getCursor(0, 0);
     * {
     *   const ans = doc.getCursorPos(pos0!);
     *   expect(ans.offset).toBe(0);
     * }
     * text.insert(0, "1");
     * {
     *   const ans = doc.getCursorPos(pos0!);
     *   expect(ans.offset).toBe(1);
     * }
     * ```
     */
    getCursor(pos: number, side?: Side): Cursor | undefined;
}

export type Side = -1 | 0 | 1;
"#;

#[wasm_bindgen(typescript_custom_section)]
const JSON_SCHEMA_TYPES: &'static str = r#"
export type JsonOpID = `${number}@${PeerID}`;
export type JsonContainerID =  `🦜:${ContainerID}` ;
export type JsonValue  =
  | JsonContainerID
  | string
  | number
  | boolean
  | null
  | { [key: string]: JsonValue }
  | Uint8Array
  | JsonValue[];

export type JsonSchema = {
  schema_version: number;
  start_version: Map<string, number>,
  peers: PeerID[],
  changes: JsonChange[]
};

export type JsonChange = {
  id: JsonOpID
  /**
   * The timestamp in seconds.
   *
   * [Unix time](https://en.wikipedia.org/wiki/Unix_time)
   * It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
   */
  timestamp: number,
  deps: JsonOpID[],
  lamport: number,
  msg: string | null,
  ops: JsonOp[]
}

export interface TextUpdateOptions {
    timeoutMs?: number,
    useRefinedDiff?: boolean,
}

export type ExportMode = {
    mode: "update",
    from?: VersionVector,
} | {
    mode: "snapshot",
} | {
    mode: "shallow-snapshot",
    frontiers: Frontiers,
} | {
    mode: "updates-in-range",
    spans: {
        id: ID,
        len: number,
    }[],
};

export type JsonOp = {
  container: ContainerID,
  counter: number,
  content: ListOp | TextOp | MapOp | TreeOp | MovableListOp | UnknownOp
}

export type ListOp = {
  type: "insert",
  pos: number,
  value: JsonValue
} | {
  type: "delete",
  pos: number,
  len: number,
  start_id: JsonOpID,
};

export type MovableListOp = {
  type: "insert",
  pos: number,
  value: JsonValue
} | {
  type: "delete",
  pos: number,
  len: number,
  start_id: JsonOpID,
}| {
  type: "move",
  from: number,
  to: number,
  elem_id: JsonOpID,
}|{
  type: "set",
  elem_id: JsonOpID,
  value: JsonValue
}

export type TextOp = {
  type: "insert",
  pos: number,
  text: string
} | {
  type: "delete",
  pos: number,
  len: number,
  start_id: JsonOpID,
} | {
  type: "mark",
  start: number,
  end: number,
  style_key: string,
  style_value: JsonValue,
  info: number
}|{
  type: "mark_end"
};

export type MapOp = {
  type: "insert",
  key: string,
  value: JsonValue
} | {
  type: "delete",
  key: string,
};

export type TreeOp = {
  type: "create",
  target: TreeID,
  parent: TreeID | undefined,
  fractional_index: string
}|{
  type: "move",
  target: TreeID,
  parent: TreeID | undefined,
  fractional_index: string
}|{
  type: "delete",
  target: TreeID
};

export type UnknownOp = {
  type: "unknown"
  prop: number,
  value_type: "unknown",
  value: {
    kind: number,
    data: Uint8Array
  }
};

export type CounterSpan = { start: number, end: number };

export type ImportStatus = {
  success: Map<PeerID, CounterSpan>,
  pending: Map<PeerID, CounterSpan> | null
}

export type Frontiers = OpId[];

/**
 * Represents a path to identify the exact location of an event's target.
 * The path is composed of numbers (e.g., indices of a list container) strings
 * (e.g., keys of a map container) and TreeID (the node of a tree container),
 * indicating the absolute position of the event's source within a loro document.
 */
export type Path = (number | string | TreeID)[];

/**
 * A batch of events that created by a single `import`/`transaction`/`checkout`.
 *
 * @prop by - How the event is triggered.
 * @prop origin - (Optional) Provides information about the origin of the event.
 * @prop diff - Contains the differential information related to the event.
 * @prop target - Identifies the container ID of the event's target.
 * @prop path - Specifies the absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
 */
export interface LoroEventBatch {
    /**
     * How the event is triggered.
     *
     * - `local`: The event is triggered by a local transaction.
     * - `import`: The event is triggered by an import operation.
     * - `checkout`: The event is triggered by a checkout operation.
     */
    by: "local" | "import" | "checkout";
    origin?: string;
    /**
     * The container ID of the current event receiver.
     * It's undefined if the subscriber is on the root document.
     */
    currentTarget?: ContainerID;
    events: LoroEvent[];
}

/**
 * The concrete event of Loro.
 */
export interface LoroEvent {
    /**
     * The container ID of the event's target.
     */
    target: ContainerID;
    diff: Diff;
    /**
     * The absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
     */
    path: Path;
}

export type ListDiff = {
    type: "list";
    diff: Delta<(Value | Container)[]>[];
};

export type TextDiff = {
    type: "text";
    diff: Delta<string>[];
};

export type MapDiff = {
    type: "map";
    updated: Record<string, Value | Container | undefined>;
};

export type TreeDiffItem =
    | {
        target: TreeID;
        action: "create";
        parent: TreeID | undefined;
        index: number;
        fractionalIndex: string;
    }
    | {
        target: TreeID;
        action: "delete";
        oldParent: TreeID | undefined;
        oldIndex: number;
    }
    | {
        target: TreeID;
        action: "move";
        parent: TreeID | undefined;
        index: number;
        fractionalIndex: string;
        oldParent: TreeID | undefined;
        oldIndex: number;
    };

export type TreeDiff = {
    type: "tree";
    diff: TreeDiffItem[];
};

export type CounterDiff = {
    type: "counter";
    increment: number;
};

export type Diff = ListDiff | TextDiff | MapDiff | TreeDiff | CounterDiff;
export type Subscription = () => void;
type NonNullableType<T> = Exclude<T, null | undefined>;
export type AwarenessListener = (
    arg: { updated: PeerID[]; added: PeerID[]; removed: PeerID[] },
    origin: "local" | "timeout" | "remote" | string,
) => void;


interface Listener {
    (event: LoroEventBatch): void;
}

interface LoroDoc {
    subscribe(listener: Listener): Subscription;
}

interface UndoManager {
    /**
     * Set the callback function that is called when an undo/redo step is pushed.
     * The function can return a meta data value that will be attached to the given stack item.
     *
     * @param listener - The callback function.
     */
    setOnPush(listener?: UndoConfig["onPush"]): void;
    /**
     * Set the callback function that is called when an undo/redo step is popped.
     * The function will have a meta data value that was attached to the given stack item when `onPush` was called.
     *
     * @param listener - The callback function.
     */
    setOnPop(listener?: UndoConfig["onPop"]): void;
}
interface LoroDoc<T extends Record<string, Container> = Record<string, Container>> {
    /**
     * Get a LoroMap by container id
     *
     * The object returned is a new js object each time because it need to cross
     * the WASM boundary.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const map = doc.getMap("map");
     * ```
     */
    getMap<Key extends keyof T | ContainerID>(name: Key): T[Key] extends LoroMap ? T[Key] : LoroMap;
    /**
     * Get a LoroList by container id
     *
     * The object returned is a new js object each time because it need to cross
     * the WASM boundary.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const list = doc.getList("list");
     * ```
     */
    getList<Key extends keyof T | ContainerID>(name: Key): T[Key] extends LoroList ? T[Key] : LoroList;
    /**
     * Get a LoroMovableList by container id
     *
     * The object returned is a new js object each time because it need to cross
     * the WASM boundary.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const list = doc.getList("list");
     * ```
     */
    getMovableList<Key extends keyof T | ContainerID>(name: Key): T[Key] extends LoroMovableList ? T[Key] : LoroMovableList;
    /**
     * Get a LoroTree by container id
     *
     *  The object returned is a new js object each time because it need to cross
     *  the WASM boundary.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const tree = doc.getTree("tree");
     *  ```
     */
    getTree<Key extends keyof T | ContainerID>(name: Key): T[Key] extends LoroTree ? T[Key] : LoroTree;
    getText(key: string | ContainerID): LoroText;
}
interface LoroList<T = unknown> {
    new(): LoroList<T>;
    /**
     *  Get elements of the list. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  list.insertContainer(3, new LoroText());
     *  console.log(list.value);  // [100, "foo", true, LoroText];
     *  ```
     */
    toArray(): T[];
    /**
     * Insert a container at the index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  const text = list.insertContainer(1, new LoroText());
     *  text.insert(0, "Hello");
     *  console.log(list.toJSON());  // [100, "Hello"];
     *  ```
     */
    insertContainer<C extends Container>(pos: number, child: C): T extends C ? T : C;
    /**
     * Push a container to the end of the list.
     */
    pushContainer<C extends Container>(child: C): T extends C ? T : C;
    /**
     * Get the value at the index. If the value is a container, the corresponding handler will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  console.log(list.get(0));  // 100
     *  console.log(list.get(1));  // undefined
     *  ```
     */
    get(index: number): T;
    /**
     *  Insert a value at index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  console.log(list.value);  // [100, "foo", true];
     *  ```
     */
    insert<V extends T>(pos: number, value: Exclude<V, Container>): void;
    delete(pos: number, len: number): void;
    push<V extends T>(value: Exclude<V, Container>): void;
    subscribe(listener: Listener): Subscription;
    getAttached(): undefined | LoroList<T>;
}
interface LoroMovableList<T = unknown> {
    new(): LoroMovableList<T>;
    /**
     *  Get elements of the list. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  list.insertContainer(3, new LoroText());
     *  console.log(list.value);  // [100, "foo", true, LoroText];
     *  ```
     */
    toArray(): T[];
    /**
     * Insert a container at the index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  const text = list.insertContainer(1, new LoroText());
     *  text.insert(0, "Hello");
     *  console.log(list.toJSON());  // [100, "Hello"];
     *  ```
     */
    insertContainer<C extends Container>(pos: number, child: C): T extends C ? T : C;
    /**
     * Push a container to the end of the list.
     */
    pushContainer<C extends Container>(child: C): T extends C ? T : C;
    /**
     * Get the value at the index. If the value is a container, the corresponding handler will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  console.log(list.get(0));  // 100
     *  console.log(list.get(1));  // undefined
     *  ```
     */
    get(index: number): T;
    /**
     *  Insert a value at index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  console.log(list.value);  // [100, "foo", true];
     *  ```
     */
    insert<V extends T>(pos: number, value: Exclude<V, Container>): void;
    delete(pos: number, len: number): void;
    push<V extends T>(value: Exclude<V, Container>): void;
    subscribe(listener: Listener): Subscription;
    getAttached(): undefined | LoroMovableList<T>;
    /**
     *  Set the value at the given position.
     *
     *  It's different from `delete` + `insert` that it will replace the value at the position.
     *
     *  For example, if you have a list `[1, 2, 3]`, and you call `set(1, 100)`, the list will be `[1, 100, 3]`.
     *  If concurrently someone call `set(1, 200)`, the list will be `[1, 200, 3]` or `[1, 100, 3]`.
     *
     *  But if you use `delete` + `insert` to simulate the set operation, they may create redundant operations
     *  and the final result will be `[1, 100, 200, 3]` or `[1, 200, 100, 3]`.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  list.set(1, "bar");
     *  console.log(list.value);  // [100, "bar", true];
     *  ```
     */
    set<V extends T>(pos: number, value: Exclude<V, Container>): void;
    /**
     * Set a container at the index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  const text = list.setContainer(0, new LoroText());
     *  text.insert(0, "Hello");
     *  console.log(list.toJSON());  // ["Hello"];
     *  ```
     */
    setContainer<C extends Container>(pos: number, child: C): T extends C ? T : C;
}
interface LoroMap<T extends Record<string, unknown> = Record<string, unknown>> {
    new(): LoroMap<T>;
    /**
     *  Get the value of the key. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  The object returned is a new js object each time because it need to cross
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  const bar = map.get("foo");
     *  ```
     */
    getOrCreateContainer<C extends Container>(key: string, child: C): C;
    /**
     * Set the key with a container.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText, LoroList } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  const text = map.setContainer("text", new LoroText());
     *  const list = map.setContainer("list", new LoroList());
     *  ```
     */
    setContainer<C extends Container, Key extends keyof T>(key: Key, child: C): NonNullableType<T[Key]> extends C ? NonNullableType<T[Key]> : C;
    /**
     *  Get the value of the key. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  The object/value returned is a new js object/value each time because it need to cross
     *  the WASM boundary.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  const bar = map.get("foo");
     *  ```
     */
    get<Key extends keyof T>(key: Key): T[Key];
    /**
     * Set the key with the value.
     *
     *  If the value of the key is exist, the old value will be updated.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  map.set("foo", "baz");
     *  ```
     */
    set<Key extends keyof T, V extends T[Key]>(key: Key, value: Exclude<V, Container>): void;
    delete(key: string): void;
    subscribe(listener: Listener): Subscription;
}
interface LoroText {
    new(): LoroText;
    insert(pos: number, text: string): void;
    delete(pos: number, len: number): void;
    subscribe(listener: Listener): Subscription;
    /**
     * Update the current text to the target text.
     *
     * It will calculate the minimal difference and apply it to the current text.
     * It uses Myers' diff algorithm to compute the optimal difference.
     *
     * This could take a long time for large texts (e.g. > 50_000 characters).
     * In that case, you should use `updateByLine` instead.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const text = doc.getText("text");
     * text.insert(0, "Hello");
     * text.update("Hello World");
     * console.log(text.toString()); // "Hello World"
     * ```
     */
    update(text: string, options?: TextUpdateOptions): void;
    /**
     * Update the current text based on the provided text.
     * This update calculation is line-based, which will be more efficient but less precise.
     */
    updateByLine(text: string, options?: TextUpdateOptions): void;
}
interface LoroTree<T extends Record<string, unknown> = Record<string, unknown>> {
    new(): LoroTree<T>;
    /**
     * Create a new tree node as the child of parent and return a `LoroTreeNode` instance.
     * If the parent is undefined, the tree node will be a root node.
     *
     * If the index is not provided, the new node will be appended to the end.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const tree = doc.getTree("tree");
     * const root = tree.createNode();
     * const node = tree.createNode(undefined, 0);
     *
     * //  undefined
     * //    /   \
     * // node  root
     * ```
     */
    createNode(parent?: TreeID, index?: number): LoroTreeNode<T>;
    move(target: TreeID, parent?: TreeID, index?: number): void;
    delete(target: TreeID): void;
    has(target: TreeID): boolean;
    /**
     * Get LoroTreeNode by the TreeID.
     */
    getNodeByID(target: TreeID): LoroTreeNode<T>;
    subscribe(listener: Listener): Subscription;
}
interface LoroTreeNode<T extends Record<string, unknown> = Record<string, unknown>> {
    /**
     * Get the associated metadata map container of a tree node.
     */
    readonly data: LoroMap<T>;
    /**
     * Create a new node as the child of the current node and
     * return an instance of `LoroTreeNode`.
     *
     * If the index is not provided, the new node will be appended to the end.
     *
     * @example
     * ```typescript
     * import { LoroDoc } from "loro-crdt";
     *
     * let doc = new LoroDoc();
     * let tree = doc.getTree("tree");
     * let root = tree.createNode();
     * let node = root.createNode();
     * let node2 = root.createNode(0);
     * //    root
     * //    /  \
     * // node2 node
     * ```
     */
    createNode(index?: number): LoroTreeNode<T>;
    move(parent?: LoroTreeNode<T>, index?: number): void;
    parent(): LoroTreeNode<T> | undefined;
    /**
     * Get the children of this node.
     *
     * The objects returned are new js objects each time because they need to cross
     * the WASM boundary.
     */
    children(): Array<LoroTreeNode<T>> | undefined;
    toJSON(): TreeNodeJSON<T>;
}
interface AwarenessWasm<T extends Value = Value> {
    getState(peer: PeerID): T | undefined;
    getTimestamp(peer: PeerID): number | undefined;
    getAllStates(): Record<PeerID, T>;
    setLocalState(value: T): void;
    removeOutdated(): PeerID[];
}

"#;
