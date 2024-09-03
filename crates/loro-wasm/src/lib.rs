//! Loro WASM bindings.
#![allow(non_snake_case)]
#![allow(clippy::empty_docs)]
#![allow(clippy::doc_lazy_continuation)]
#![warn(missing_docs)]

use convert::resolved_diff_to_js;
use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use loro_internal::{
    change::Lamport,
    configure::{StyleConfig, StyleConfigMap},
    container::{richtext::ExpandType, ContainerID},
    cursor::{self, Side},
    encoding::ImportBlobMetadata,
    event::Index,
    handler::{
        Handler, ListHandler, MapHandler, TextDelta, TextHandler, TreeHandler, ValueOrHandler,
    },
    id::{Counter, PeerID, TreeID, ID},
    json::JsonSchema,
    loro::CommitOptions,
    loro_common::check_root_container_name,
    obs::SubID,
    undo::{UndoItemMeta, UndoOrRedo},
    version::Frontiers,
    ContainerType, DiffEvent, FxHashMap, HandlerTrait, LoroDoc as LoroDocInner, LoroValue,
    MovableListHandler, UndoManager as InnerUndoManager, VersionVector as InternalVersionVector,
};
use rle::HasLength;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, cmp::Ordering, rc::Rc, sync::Arc};
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
/// import { LoroDoc } import "loro-crdt"
///
/// const loro = new LoroDoc();
/// const text = loro.getText("text");
/// const list = loro.getList("list");
/// const map = loro.getMap("Map");
/// const tree = loro.getTree("tree");
/// ```
///
// When FinalizationRegistry is unavailable, it's the users' responsibility to free the document.
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

fn js_id_to_id(id: JsID) -> Result<ID, JsValue> {
    let peer = js_peer_to_peer(Reflect::get(&id, &"peer".into())?)?;
    let counter = Reflect::get(&id, &"counter".into())?.as_f64().unwrap() as Counter;
    let id = ID::new(peer, counter);
    Ok(id)
}

fn frontiers_to_ids(frontiers: &Frontiers) -> JsIDs {
    let js_arr = Array::new();
    for id in frontiers.iter() {
        let value = id_to_js(id);
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
    /// New document will have random peer id.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let doc = LoroDocInner::new();
        doc.start_auto_commit();
        Self(Arc::new(doc))
    }

    /// Set whether to record the timestamp of each change. Default is `false`.
    ///
    /// If enabled, the Unix timestamp will be recorded for each change automatically.
    ///
    /// You can also set each timestamp manually when you commit a change.
    /// The timestamp manually set will override the automatic one.
    ///
    /// NOTE: Timestamps are forced to be in ascending order.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    #[wasm_bindgen(js_name = "setRecordTimestamp")]
    pub fn set_record_timestamp(&self, auto_record: bool) {
        self.0.set_record_timestamp(auto_record);
    }

    /// If two continuous local changes are within the interval, they will be merged into one change.
    ///
    /// The default value is 1_000_000, the default unit is milliseconds.
    #[wasm_bindgen(js_name = "setChangeMergeInterval")]
    pub fn set_change_merge_interval(&self, interval: f64) {
        self.0.set_change_merge_interval(interval as i64);
    }

    /// Set the jitter of the tree position(Fractional Index).
    ///
    /// The jitter is used to avoid conflicts when multiple users are creating the node at the same position.
    /// value 0 is default, which means no jitter, any value larger than 0 will enable jitter.
    /// Generally speaking, jitter will affect the growth rate of document size.
    #[wasm_bindgen(js_name = "setFractionalIndexJitter")]
    pub fn set_fractional_index_jitter(&self, jitter: u8) {
        self.0.set_fractional_index_jitter(jitter);
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

    /// Get a loro document from the snapshot.
    ///
    /// @see You can check out what is the snapshot [here](#).
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } import "loro-crdt"
    ///
    /// const bytes = /* The bytes encoded from other loro document *\/;
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
    /// This method has the same effect as invoking `checkout_to_latest`.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// loro.checkout(frontiers);
    /// // you need call `attach()` or `checkoutToLatest()` before changing the doc.
    /// loro.attach();
    /// text.insert(0, "Hi");
    /// ```
    pub fn attach(&mut self) {
        self.0.attach();
    }

    /// `detached` indicates that the `DocState` is not synchronized with the latest version of `OpLog`.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// When `detached`, the document is not editable.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// console.log(doc.is_detached());  // false
    /// loro.checkout(frontiers);
    /// console.log(doc.is_detached());  // true
    /// loro.attach();
    /// console.log(doc.is_detached());  // false
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
    /// console.log(doc.is_detached());  // true
    /// ```
    pub fn detach(&self) {
        self.0.detach()
    }

    /// Duplicate the document with a different PeerID
    ///
    /// The time complexity and space complexity of this operation are both O(n),
    pub fn fork(&self) -> Self {
        Self(Arc::new(self.0.fork()))
    }

    /// Checkout the `DocState` to the latest version of `OpLog`.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
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
    /// loro.checkout(frontiers);
    /// // you need call `checkoutToLatest()` or `attach()` before changing the doc.
    /// loro.checkoutToLatest();
    /// text.insert(0, "Hi");
    /// ```
    #[wasm_bindgen(js_name = "checkoutToLatest")]
    pub fn checkout_to_latest(&mut self) -> JsResult<()> {
        self.0.checkout_to_latest();
        Ok(())
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
    /// loro.checkout(frontiers);
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
    /// You can specify the `origin` and `timestamp` of the commit.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// NOTE: Timestamps are forced to be in ascending order.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    pub fn commit(&self, origin: Option<String>, timestamp: Option<f64>) {
        let mut options = CommitOptions::default();
        options.set_origin(origin.as_deref());
        options.set_timestamp(timestamp.map(|x| x as i64));
        self.0.commit_with(options);
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

    /// Get the encoded version vector of the current document.
    ///
    /// If you checkout to a specific version, the version vector will change.
    #[inline(always)]
    pub fn version(&self) -> VersionVector {
        VersionVector(self.0.state_vv())
    }

    /// Get the encoded version vector of the latest version in OpLog.
    ///
    /// If you checkout to a specific version, the version vector will not change.
    #[wasm_bindgen(js_name = "oplogVersion")]
    pub fn oplog_version(&self) -> VersionVector {
        VersionVector(self.0.oplog_vv())
    }

    /// Get the frontiers of the current document.
    ///
    /// If you checkout to a specific version, this value will change.
    #[inline]
    pub fn frontiers(&self) -> JsIDs {
        frontiers_to_ids(&self.0.state_frontiers())
    }

    /// Get the frontiers of the latest version in OpLog.
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

    /// Export the snapshot of current version, it's include all content of
    /// operations and states
    #[wasm_bindgen(js_name = "exportSnapshot")]
    pub fn export_snapshot(&self) -> JsResult<Vec<u8>> {
        Ok(self.0.export_snapshot())
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

    /// Export updates from the specific version to the current version with JSON format.
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
    pub fn import_json_updates(&self, json: JsJsonSchemaOrString) -> JsResult<()> {
        let json: JsValue = json.into();
        if JsValue::is_string(&json) {
            let json_str = json.as_string().unwrap();
            return self
                .0
                .import_json_updates(json_str.as_str())
                .map_err(|e| e.into());
        }
        let json_schema: JsonSchema = serde_wasm_bindgen::from_value(json)?;
        self.0.import_json_updates(json_schema)?;
        Ok(())
    }

    /// Import a snapshot or a update to current doc.
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
    /// const updates = doc.exportFrom();
    /// const snapshot = doc.exportSnapshot();
    /// const doc2 = new LoroDoc();
    /// // import snapshot
    /// doc2.import(snapshot);
    /// // or import updates
    /// doc2.import(updates);
    /// ```
    pub fn import(&self, update_or_snapshot: &[u8]) -> JsResult<()> {
        self.0.import(update_or_snapshot)?;
        Ok(())
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
    /// const updates = doc.exportFrom();
    /// const snapshot = doc.exportSnapshot();
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

    /// Get the json format of the document state.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
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
    /// transaction is committed or updates from remote are imported.
    ///
    /// Returns a subscription ID, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// doc.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// text.insert(0, "Hello");
    /// // the events will be emitted when `commit()` is called.
    /// doc.commit();
    /// ```
    // TODO: convert event and event sub config
    pub fn subscribe(&self, f: js_sys::Function) -> u32 {
        let observer = observer::Observer::new(f);
        let doc = self.0.clone();
        self.0
            .subscribe_root(Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc)
                // call_subscriber(observer.clone(), e);
            }))
            .into_u32()
    }

    /// Unsubscribe by the subscription id.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// const subscription = doc.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// text.insert(0, "Hello");
    /// // the events will be emitted when `commit()` is called.
    /// doc.commit();
    /// doc.unsubscribe(subscription);
    /// ```
    pub fn unsubscribe(&self, subscription: u32) {
        self.0.unsubscribe(SubID::from_u32(subscription))
    }

    /// Debug the size of the history
    #[wasm_bindgen(js_name = "debugHistory")]
    pub fn debug_history(&self) {
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().lock().unwrap();
        console_log!("{:#?}", oplog.diagnose_size());
    }

    /// Get all of changes in the oplog
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const changes = doc.getAllChanges();
    ///
    /// for (let [peer, changes] of changes.entries()){
    ///     console.log("peer: ", peer);
    ///     for (let change in changes){
    ///         console.log("change: ", change);
    ///     }
    /// }
    /// ```
    #[wasm_bindgen(js_name = "getAllChanges")]
    pub fn get_all_changes(&self) -> JsChanges {
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().lock().unwrap();
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

    /// Get the change of a specific ID
    #[wasm_bindgen(js_name = "getChangeAt")]
    pub fn get_change_at(&self, id: JsID) -> JsResult<JsChange> {
        let id = js_id_to_id(id)?;
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().lock().unwrap();
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
        let oplog = borrow_mut.oplog().lock().unwrap();
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
        };
        Ok(change.to_js().into())
    }

    /// Get all ops of the change of a specific ID
    #[wasm_bindgen(js_name = "getOpsInChange")]
    pub fn get_ops_in_change(&self, id: JsID) -> JsResult<Vec<JsValue>> {
        let id = js_id_to_id(id)?;
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().lock().unwrap();
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

    /// Convert frontiers to a readable version vector
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
        let f = self.0.oplog().lock().unwrap().dag().vv_to_frontiers(&vv.0);
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

fn convert_container_path_to_js_value(path: &[(ContainerID, Index)]) -> JsValue {
    let arr = Array::new_with_length(path.len() as u32);
    for (i, p) in path.iter().enumerate() {
        arr.set(i as u32, p.1.clone().into());
    }
    let path: JsValue = arr.into_js_result().unwrap();
    path
}

/// The handler of a text container. It supports rich text CRDT.
///
/// ## Updating Text Content Using a Diff Algorithm
///
/// A common requirement is to update the current text to a target text.
/// You can implement this using a text diff algorithm of your choice.
/// Below is a sample you can directly copy into your code, which uses the
/// [fast-diff](https://www.npmjs.com/package/fast-diff) package.
///
/// ```ts
/// import { diff } from "fast-diff";
/// import { LoroText } from "loro-crdt";
///
/// function updateText(text: LoroText, newText: string) {
///   const src = text.toString();
///   const delta = diff(src, newText);
///   let index = 0;
///   for (const [op, text] of delta) {
///     if (op === 0) {
///     index += text.length;
///   } else if (op === 1) {
///     text.insert(index, text);
///     index += text.length;
///   } else {
///     text.delete(index, text.length);
///   }
/// }
/// ```
///
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
    /// Create a new detached LoroText.
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

    /// Iterate each span(internal storage unit) of the text.
    ///
    /// The callback function will be called for each span in the text.
    /// If the callback returns `false`, the iteration will stop.
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

    /// Update the current text based on the provided text.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// text.update("Hello World");
    /// ```
    pub fn update(&self, text: &str) {
        self.handler.update(text);
    }

    /// Insert some string at index.
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

    /// Get a string slice.
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

    /// Get the character at the given position.
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

    /// Delete and return the string at the given range and insert a string at the same position.
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

    /// Delete elements from index to index + len
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

    /// Mark a range of text with a key and a value.
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

    /// Unmark a range of text with a key and a value.
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

    /// Convert the state to string
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

    /// Get the length of text
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.handler.len_utf16()
    }

    /// Subscribe to the changes of the text.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
    /// - `doc.import(data)` is called.
    /// - `doc.checkout(version)` is called.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<u32> {
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

        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription id.
    pub fn unsubscribe(&self, subscription: u32) -> JsResult<()> {
        self.doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?
            .unsubscribe(SubID::from_u32(subscription));
        Ok(())
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
    /// import { LoroDoc } from "loro-crdt";
    ///
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

    /// get the cursor at the given position.
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
    /// Create a new detached LoroMap.
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
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const text = map.setContainer("text", new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(map.getDeepValue());  // {"foo": "bar", "text": "Hello"}
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsValue {
        self.handler.get_deep_value().into()
    }

    /// Set the key with a container.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
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
    /// Returns a subscription id, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
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
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let doc = self
            .doc
            .clone()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = doc.clone();
        let id = doc.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );

        Ok(id.into_u32())
    }

    /// Unsubscribe by the subscription.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const map = doc.getMap("map");
    /// const subscription = map.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// map.set("foo", "bar");
    /// doc.commit();
    /// map.unsubscribe(subscription);
    /// ```
    pub fn unsubscribe(&self, subscription: u32) -> JsResult<()> {
        self.doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?
            .unsubscribe(SubID::from_u32(subscription));
        Ok(())
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
    /// Create a new detached LoroList.
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
    /// import { LoroDoc } from "loro-crdt";
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
        self.handler.for_each(|(_, x)| {
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
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.getDeepValue());  // [100, "Hello"];
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
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.getDeepValue());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "insertContainer", skip_typescript)]
    pub fn insert_container(&mut self, index: usize, child: JsContainer) -> JsResult<JsContainer> {
        let child = js_to_container(child)?;
        let c = self.handler.insert_container(index, child.to_handler())?;
        Ok(handler_to_js_value(c, self.doc.clone()).into())
    }

    /// Subscribe to the changes of the list.
    ///
    /// Returns a subscription id, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
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
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<u32> {
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
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription id.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// const subscription = list.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// list.insert(0, 100);
    /// doc.commit();
    /// list.unsubscribe(subscription);
    /// ```
    pub fn unsubscribe(&self, subscription: u32) -> JsResult<()> {
        self.doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?
            .unsubscribe(SubID::from_u32(subscription));
        Ok(())
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
    /// Create a new detached LoroList.
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
    /// import { LoroDoc } from "loro-crdt";
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
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.getDeepValue());  // [100, "Hello"];
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
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, new LoroText());
    /// text.insert(0, "Hello");
    /// console.log(list.getDeepValue());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "insertContainer", skip_typescript)]
    pub fn insert_container(&mut self, index: usize, child: JsContainer) -> JsResult<JsContainer> {
        let child = js_to_container(child)?;
        let c = self.handler.insert_container(index, child.to_handler())?;
        Ok(handler_to_js_value(c, self.doc.clone()).into())
    }

    /// Subscribe to the changes of the list.
    ///
    /// Returns a subscription id, which can be used to unsubscribe.
    ///
    /// The events will be emitted after a transaction is committed. A transaction is committed when:
    ///
    /// - `doc.commit()` is called.
    /// - `doc.exportFrom(version)` is called.
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
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let loro = self
            .doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let doc_clone = loro.clone();
        let ans = loro.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, &doc_clone);
            }),
        );
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription id.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const list = doc.getList("list");
    /// const subscription = list.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// list.insert(0, 100);
    /// doc.commit();
    /// list.unsubscribe(subscription);
    /// ```
    pub fn unsubscribe(&self, subscription: u32) -> JsResult<()> {
        self.doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?
            .unsubscribe(SubID::from_u32(subscription));
        Ok(())
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
            self.tree.create_at(Some(self.id), index)?
        } else {
            self.tree.create(Some(self.id))?
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
    /// const root = tree.createChildNode();
    /// const node = root.createChildNode();
    /// const node2 = node.createChildNode();
    /// node2.moveTo(undefined, 0);
    /// // node2   root
    /// //          |
    /// //         node
    ///
    /// ```
    #[wasm_bindgen(js_name = "move")]
    pub fn mov(&self, parent: &JsTreeNodeOrUndefined, index: Option<usize>) -> JsResult<()> {
        let parent: Option<LoroTreeNode> = parse_js_tree_node(parent)?;
        if let Some(index) = index {
            self.tree.move_to(self.id, parent.map(|x| x.id), index)?
        } else {
            self.tree.mov(self.id, parent.map(|x| x.id))?;
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

    /// Get the parent node of this node.
    ///
    /// - The parent of the root node is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    #[wasm_bindgen]
    pub fn parent(&self) -> Option<LoroTreeNode> {
        let parent = self.tree.get_node_parent(&self.id).flatten();
        parent.map(|p| LoroTreeNode::from_tree(p, self.tree.clone(), self.doc.clone()))
    }

    /// Get the children of this node.
    ///
    /// The objects returned are new js objects each time because they need to cross
    /// the WASM boundary.
    #[wasm_bindgen(skip_typescript)]
    pub fn children(&self) -> JsValue {
        let Some(children) = self.tree.children(Some(self.id)) else {
            return JsValue::undefined();
        };
        let children = children.into_iter().map(|c| {
            let node = LoroTreeNode::from_tree(c, self.tree.clone(), self.doc.clone());
            JsValue::from(node)
        });
        Array::from_iter(children).into()
    }
}

#[wasm_bindgen]
impl LoroTree {
    /// Create a new detached LoroTree.
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
            self.handler.create_at(parent, index)?
        } else {
            self.handler.create(parent)?
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
    /// tree.move(node2, root);
    /// // Error will be thrown if move operation creates a cycle
    /// tree.move(root, node);
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
            self.handler.move_to(target, parent, index)?
        } else {
            self.handler.mov(target, parent)?
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
        if self.handler.contains(target) {
            Some(LoroTreeNode::from_tree(
                target,
                self.handler.clone(),
                self.doc.clone(),
            ))
        } else {
            None
        }
    }

    /// Get the id of the container.
    #[wasm_bindgen(js_name = "id", getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Return `true` if the tree contains the TreeID, `false` if the target is deleted or wrong.
    #[wasm_bindgen(js_name = "has")]
    pub fn contains(&self, target: &JsTreeID) -> bool {
        let target: JsValue = target.into();
        self.handler.contains(target.try_into().unwrap())
    }

    /// Get the flat array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use `toJson()`.
    ///
    // TODO: perf
    #[wasm_bindgen(js_name = "toArray", skip_typescript)]
    pub fn to_array(&mut self) -> JsResult<Array> {
        let value = self.handler.get_value().into_list().unwrap();
        let ans = Array::new();
        for v in value.as_ref() {
            let v = v.as_map().unwrap();
            let id: JsValue = TreeID::try_from(v["id"].as_string().unwrap().as_str())
                .unwrap()
                .into();
            let id: JsTreeID = id.into();
            let parent = if let LoroValue::String(p) = &v["parent"] {
                Some(TreeID::try_from(p.as_str())?)
            } else {
                None
            };
            let parent: JsParentTreeID = parent
                .map(|x| LoroTreeNode::from_tree(x, self.handler.clone(), self.doc.clone()).into())
                .unwrap_or(JsValue::undefined())
                .into();
            let index = *v["index"].as_i64().unwrap() as u32;
            let position = v["fractional_index"].as_string().unwrap();
            let map: LoroMap = self.get_node_by_id(&id).unwrap().data()?;
            let obj = Object::new();
            js_sys::Reflect::set(&obj, &"id".into(), &id)?;
            js_sys::Reflect::set(&obj, &"parent".into(), &parent)?;
            js_sys::Reflect::set(&obj, &"index".into(), &JsValue::from(index))?;
            js_sys::Reflect::set(
                &obj,
                &"fractional_index".into(),
                &JsValue::from_str(position),
            )?;
            js_sys::Reflect::set(&obj, &"meta".into(), &map.into())?;
            ans.push(&obj);
        }
        Ok(ans)
    }

    /// Get the flat array with metadata of the forest.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const root = tree.createNode();
    /// root.data.set("color", "red");
    /// // [ { id: '0@F2462C4159C4C8D1', parent: null, meta: { color: 'red' } } ]
    /// console.log(tree.toJSON());
    /// ```
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> JsValue {
        self.handler.get_deep_value().into()
    }

    /// Get all tree ids of the forest.
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
    /// Returns a subscription id, which can be used to unsubscribe.
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
    /// - `doc.exportFrom(version)` is called.
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
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<u32> {
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
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription id.
    ///
    /// @example
    /// ```ts
    /// import { LoroDoc } from "loro-crdt";
    ///
    /// const doc = new LoroDoc();
    /// const tree = doc.getTree("tree");
    /// const subscription = tree.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// const root = tree.createNode();
    /// const node = root.createNode();
    /// doc.commit();
    /// tree.unsubscribe(subscription);
    /// ```
    pub fn unsubscribe(&self, subscription: u32) -> JsResult<()> {
        self.doc
            .as_ref()
            .ok_or_else(|| JsError::new("Document is not attached"))?
            .unsubscribe(SubID::from_u32(subscription));
        Ok(())
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
/// - isSnapshot
/// - changeNum
#[wasm_bindgen(js_name = "decodeImportBlobMeta")]
pub fn decode_import_blob_meta(blob: &[u8]) -> JsResult<JsImportBlobMetadata> {
    let meta: ImportBlobMetadata = LoroDocInner::decode_import_blob_meta(blob)?;
    Ok(meta.into())
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
/**
* Container types supported by loro.
*
* It is most commonly used to specify the type of sub-container to be created.
* @example
* ```ts
* import { LoroDoc } from "loro-crdt";
*
* const doc = new LoroDoc();
* const list = doc.getList("list");
* list.insert(0, 100);
* const containerType = "Text";
* const text = list.insertContainer(1, containerType);
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
    timestamp: number,
    deps: OpId[],
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
    isSnapshot: boolean;
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
}

interface LoroTree{
    toArray(): TreeNodeValue[];
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
  timestamp: number,
  deps: JsonOpID[],
  lamport: number,
  msg: string | null,
  ops: JsonOp[]
}

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
"#;
