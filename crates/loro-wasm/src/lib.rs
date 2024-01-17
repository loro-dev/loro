#![allow(non_snake_case)]
use convert::resolved_diff_to_js;
use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use loro_internal::{
    change::Lamport,
    configure::{StyleConfig, StyleConfigMap},
    container::{richtext::ExpandType, ContainerID},
    event::{Diff, Index},
    handler::{ListHandler, MapHandler, TextDelta, TextHandler, TreeHandler, ValueOrContainer},
    id::{Counter, TreeID, ID},
    obs::SubID,
    version::Frontiers,
    ContainerType, DiffEvent, LoroDoc, LoroError, LoroValue, VersionVector,
};
use rle::HasLength;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, cmp::Ordering, panic, rc::Rc, sync::Arc};
use wasm_bindgen::{__rt::IntoJsResult, prelude::*};
mod log;

use crate::convert::handler_to_js_value;

mod convert;

#[wasm_bindgen(start)]
fn run() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[wasm_bindgen(js_name = setDebug)]
pub fn set_debug(filter: &str) {
    debug_log::set_debug(filter)
}

type JsResult<T> = Result<T, JsValue>;

/// The CRDTs document. Loro supports different CRDTs include [**List**](LoroList),
/// [**RichText**](LoroText), [**Map**](LoroMap) and [**Movable Tree**](LoroTree),
/// you could build all kind of applications by these.
///
/// @example
/// ```ts
/// import { Loro } import "loro-crdt"
///
/// const loro = new Loro();
/// const text = loro.getText("text");
/// const list = loro.getList("list");
/// const map = loro.getMap("Map");
/// const tree = loro.getTree("tree");
/// ```
///
// When FinalizationRegistry is unavailable, it's the users' responsibility to free the document.
#[wasm_bindgen]
pub struct Loro(Arc<LoroDoc>);

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "ContainerID")]
    pub type JsContainerID;
    #[wasm_bindgen(typescript_type = "ContainerID | string")]
    pub type JsIntoContainerID;
    #[wasm_bindgen(typescript_type = "Transaction | Loro")]
    pub type JsTransaction;
    #[wasm_bindgen(typescript_type = "string | undefined")]
    pub type JsOrigin;
    #[wasm_bindgen(typescript_type = "{ peer: PeerID, counter: number }")]
    pub type JsID;
    #[wasm_bindgen(typescript_type = "{ start: number, end: number }")]
    pub type JsRange;
    #[wasm_bindgen(typescript_type = "number|bool|string|null")]
    pub type JsMarkValue;
    #[wasm_bindgen(typescript_type = "TreeID")]
    pub type JsTreeID;
    #[wasm_bindgen(typescript_type = "Delta<string>[]")]
    pub type JsStringDelta;
    #[wasm_bindgen(typescript_type = "Map<PeerID, number>")]
    pub type JsVersionVectorMap;
    #[wasm_bindgen(typescript_type = "Map<PeerID, Change[]>")]
    pub type JsChanges;
    #[wasm_bindgen(typescript_type = "Change")]
    pub type JsChange;
    #[wasm_bindgen(typescript_type = "Map<PeerID, number> | Uint8Array")]
    pub type JsVersionVector;
    #[wasm_bindgen(typescript_type = "Value | Container")]
    pub type JsValueOrContainer;
    #[wasm_bindgen(typescript_type = "Value | Container | undefined")]
    pub type JsValueOrContainerOrUndefined;
    #[wasm_bindgen(typescript_type = "[string, Value | Container]")]
    pub type MapEntry;
    #[wasm_bindgen(typescript_type = "{[key: string]: { expand: 'before'|'after'|'none'|'both' }}")]
    pub type JsTextStyles;
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
                panic!("Observer called from different thread")
            }
        }
    }

    // TODO: need to double check whether this is safe
    unsafe impl Send for Observer {}
    // TODO: need to double check whether this is safe
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

fn js_id_to_id(id: JsID) -> Result<ID, JsValue> {
    let peer = Reflect::get(&id, &"peer".into())?.as_string().unwrap();
    let counter = Reflect::get(&id, &"counter".into())?.as_f64().unwrap() as Counter;
    let id = ID::new(
        peer.parse()
            .map_err(|_e| JsValue::from_str(&format!("cannot parse {} to PeerID", peer)))?,
        counter,
    );
    Ok(id)
}

fn frontiers_to_ids(frontiers: &Frontiers) -> Vec<JsID> {
    let mut ans = Vec::with_capacity(frontiers.len());
    for id in frontiers.iter() {
        let obj = Object::new();
        Reflect::set(&obj, &"peer".into(), &id.peer.to_string().into()).unwrap();
        Reflect::set(&obj, &"counter".into(), &id.counter.into()).unwrap();
        let value: JsValue = obj.into_js_result().unwrap();
        ans.push(value.into());
    }

    ans
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
    let cid = ContainerID::try_from(s.as_str())
        .unwrap_or_else(|_| ContainerID::new_root(s.as_str(), kind));
    Ok(cid)
}

fn js_value_to_version(version: &JsValue) -> Result<VersionVector, JsValue> {
    let version: Option<Vec<u8>> = if version.is_null() || version.is_undefined() {
        None
    } else {
        let arr: Uint8Array = Uint8Array::new(version);
        Some(arr.to_vec())
    };

    let vv = match version {
        Some(x) => VersionVector::decode(&x)?,
        None => Default::default(),
    };

    Ok(vv)
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
impl Loro {
    /// Create a new loro document.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut doc = LoroDoc::new();
        doc.start_auto_commit();
        Self(Arc::new(doc))
    }

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
    /// import { Loro } import "loro-crdt"
    ///
    /// const bytes = /* The bytes encoded from other loro document *\/;
    /// const loro = Loro.fromSnapshot(bytes);
    /// ```
    ///
    #[wasm_bindgen(js_name = "fromSnapshot")]
    pub fn from_snapshot(snapshot: &[u8]) -> JsResult<Loro> {
        let mut doc = LoroDoc::from_snapshot(snapshot)?;
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// console.log(doc.is_detached());  // false
    /// loro.checkout(frontiers);
    /// console.log(doc.is_detached());  // true
    /// loro.attach();
    /// console.log(doc.is_detached());  // false
    /// ```
    pub fn is_detached(&self) -> bool {
        self.0.is_detached()
    }

    /// Checkout the `DocState` to the lastest version of `OpLog`.
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// You should call `attach` to attach the `DocState` to the lastest version of `OpLog`.
    ///
    /// @param frontiers - the specific frontiers
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// const frontiers = doc.frontiers();
    /// text.insert(0, "Hello World!");
    /// loro.checkout(frontiers);
    /// console.log(doc.toJson()); // {"text": ""}
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

    /// Get peer id in hex string.
    #[wasm_bindgen(js_name = "peerIdStr", method, getter)]
    pub fn peer_id_str(&self) -> String {
        format!("{:X}", self.0.peer_id())
    }

    /// Set the peer ID of the current writer.
    ///
    /// Note: use it with caution. You need to make sure there is not chance that two peers
    /// have the same peer ID.
    #[wasm_bindgen(js_name = "setPeerId", method)]
    pub fn set_peer_id(&self, id: u64) -> JsResult<()> {
        self.0.set_peer_id(id)?;
        Ok(())
    }

    /// Commit the cumulative auto commit transaction.
    pub fn commit(&self, origin: Option<String>) {
        self.0.commit_with(origin.map(|x| x.into()), None, true);
    }

    /// Get a LoroText by container id
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// ```
    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&self, cid: &JsIntoContainerID) -> JsResult<LoroText> {
        let text = self
            .0
            .get_text(js_value_to_container_id(cid, ContainerType::Text)?);
        Ok(LoroText {
            handler: text,
            _doc: self.0.clone(),
        })
    }

    /// Get a LoroMap by container id
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// ```
    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&self, cid: &JsIntoContainerID) -> JsResult<LoroMap> {
        let map = self
            .0
            .get_map(js_value_to_container_id(cid, ContainerType::Map)?);
        Ok(LoroMap {
            handler: map,
            doc: self.0.clone(),
        })
    }

    /// Get a LoroList by container id
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// ```
    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&self, cid: &JsIntoContainerID) -> JsResult<LoroList> {
        let list = self
            .0
            .get_list(js_value_to_container_id(cid, ContainerType::List)?);
        Ok(LoroList {
            handler: list,
            doc: self.0.clone(),
        })
    }

    /// Get a LoroTree by container id
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// ```
    #[wasm_bindgen(js_name = "getTree")]
    pub fn get_tree(&self, cid: &JsIntoContainerID) -> JsResult<LoroTree> {
        let tree = self
            .0
            .get_tree(js_value_to_container_id(cid, ContainerType::Tree)?);
        Ok(LoroTree {
            handler: tree,
            doc: self.0.clone(),
        })
    }

    /// Get the container corresponding to the container id
    ///
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
                    doc: self.0.clone(),
                }
                .into()
            }
            ContainerType::List => {
                let list = self.0.get_list(container_id);
                LoroList {
                    handler: list,
                    doc: self.0.clone(),
                }
                .into()
            }
            ContainerType::Text => {
                let richtext = self.0.get_text(container_id);
                LoroText {
                    handler: richtext,
                    _doc: self.0.clone(),
                }
                .into()
            }
            ContainerType::Tree => {
                let tree = self.0.get_tree(container_id);
                LoroTree {
                    handler: tree,
                    doc: self.0.clone(),
                }
                .into()
            }
        })
    }

    /// Get the encoded version vector of the current document.
    ///
    /// If you checkout to a specific version, the version vector will change.
    #[inline(always)]
    pub fn version(&self) -> Vec<u8> {
        self.0.state_vv().encode()
    }

    /// Get the encoded version vector of the lastest version in OpLog.
    ///
    /// If you checkout to a specific version, the version vector will not change.
    #[inline(always)]
    #[wasm_bindgen(js_name = "oplogVersion")]
    pub fn oplog_version(&self) -> Vec<u8> {
        self.0.oplog_vv().encode()
    }

    /// Get the frontiers of the current document.
    ///
    /// If you checkout to a specific version, this value will change.
    #[inline]
    pub fn frontiers(&self) -> Vec<JsID> {
        frontiers_to_ids(&self.0.state_frontiers())
    }

    /// Get the frontiers of the lastest version in OpLog.
    ///
    /// If you checkout to a specific version, this value will not change.
    #[inline(always)]
    pub fn oplog_frontiers(&self) -> Vec<JsID> {
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
    #[inline]
    #[wasm_bindgen(js_name = "cmpFrontiers")]
    pub fn cmp_frontiers(&self, frontiers: Vec<JsID>) -> JsResult<i32> {
        let frontiers = ids_to_frontiers(frontiers)?;
        Ok(match self.0.cmp_frontiers(&frontiers) {
            Ordering::Less => -1,
            Ordering::Greater => 1,
            Ordering::Equal => 0,
        })
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    pub fn export_from(&self, version: &JsValue) -> JsResult<Vec<u8>> {
        // `version` may be null or undefined
        let vv = js_value_to_version(version)?;
        Ok(self.0.export_from(&vv))
    }

    /// Import a snapshot or a update to current doc.
    ///
    /// Note:
    /// - Updates within the current version will be ignored
    /// - Updates with missing dependencies will be pending until the dependencies are received
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// // get all updates of the doc
    /// const updates = doc.exportFrom();
    /// const snapshot = doc.exportSnapshot();
    /// const doc2 = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const updates = doc.exportFrom();
    /// const snapshot = doc.exportSnapshot();
    /// const doc2 = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.insert(0, "Hello");
    /// const text = list.insertContainer(0, "Text");
    /// text.insert(0, "Hello");
    /// const map = list.insertContainer(1, "Map");
    /// map.set("foo", "bar");
    /// /*
    /// {"list": ["Hello", {"foo": "bar"}]}
    ///  *\/
    /// console.log(doc.toJson());
    /// ```
    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> JsResult<JsValue> {
        let json = self.0.get_deep_value();
        Ok(json.into())
    }

    /// Subscribe to the changes of the loro document. The function will be called when the
    /// transaction is committed or updates from remote are imported.
    ///
    /// Returns a subscription ID, which can be used to unsubscribe.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
                call_after_micro_task(observer.clone(), e, doc.clone())
                // call_subscriber(observer.clone(), e);
            }))
            .into_u32()
    }

    /// Unsubscribe by the subscription
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
        let changes = oplog.changes();
        let ans = js_sys::Map::new();
        for (peer_id, changes) in changes {
            let row = js_sys::Array::new_with_length(changes.len() as u32);
            for (i, change) in changes.iter().enumerate() {
                let change = ChangeMeta {
                    lamport: change.lamport,
                    length: change.atom_len() as u32,
                    peer: change.peer().to_string(),
                    counter: change.id.counter,
                    deps: change
                        .deps
                        .iter()
                        .map(|dep| StringID {
                            peer: dep.peer.to_string(),
                            counter: dep.counter,
                        })
                        .collect(),
                    timestamp: change.timestamp as f64,
                };
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
            lamport: change.lamport,
            length: change.atom_len() as u32,
            peer: change.peer().to_string(),
            counter: change.id.counter,
            deps: change
                .deps
                .iter()
                .map(|dep| StringID {
                    peer: dep.peer.to_string(),
                    counter: dep.counter,
                })
                .collect(),
            timestamp: change.timestamp as f64,
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const frontiers = doc.frontiers();
    /// const version = doc.frontiersToVV(frontiers);
    /// ```
    #[wasm_bindgen(js_name = "frontiersToVV")]
    pub fn frontiers_to_vv(&self, frontiers: Vec<JsID>) -> JsResult<JsVersionVectorMap> {
        let frontiers = ids_to_frontiers(frontiers)?;
        let borrow_mut = &self.0;
        let oplog = borrow_mut.oplog().try_lock().unwrap();
        oplog
            .dag()
            .frontiers_to_vv(&frontiers)
            .map(|vv| {
                let ans: JsVersionVectorMap = vv_to_js_value(vv).into();
                ans
            })
            .ok_or_else(|| JsError::new("Frontiers not found").into())
    }

    /// Convert a version vector to frontiers
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// const version = doc.version();
    /// const frontiers = doc.vvToFrontiers(version);
    /// ```
    #[wasm_bindgen(js_name = "vvToFrontiers")]
    pub fn vv_to_frontiers(&self, vv: &JsVersionVector) -> JsResult<Vec<JsID>> {
        let value: JsValue = vv.into();
        let is_bytes = value.is_instance_of::<js_sys::Uint8Array>();
        let vv = if is_bytes {
            let bytes = js_sys::Uint8Array::from(value.clone());
            let bytes = bytes.to_vec();
            VersionVector::decode(&bytes)?
        } else {
            let map = js_sys::Map::from(value);
            js_map_to_vv(map)?
        };

        let f = self.0.oplog().lock().unwrap().dag().vv_to_frontiers(&vv);
        Ok(frontiers_to_ids(&f))
    }
}

fn js_map_to_vv(map: js_sys::Map) -> JsResult<VersionVector> {
    let mut vv = VersionVector::new();
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

    Ok(vv)
}

#[allow(unused)]
fn call_subscriber(ob: observer::Observer, e: DiffEvent, doc: Arc<LoroDoc>) {
    // We convert the event to js object here, so that we don't need to worry about GC.
    // In the future, when FinalizationRegistry[1] is stable, we can use `--weak-ref`[2] feature
    // in wasm-bindgen to avoid this.
    //
    // [1]: https://caniuse.com/?search=FinalizationRegistry
    // [2]: https://rustwasm.github.io/wasm-bindgen/reference/weak-references.html
    let event = Event {
        id: e.doc.id(),
        path: Event::get_path(
            e.container.path.len() as u32,
            e.container.path.iter().map(|x| &x.1),
        ),
        from_children: e.from_children,
        local: e.doc.local,
        origin: e.doc.origin.to_string(),
        target: e.container.id.clone(),
        diff: e.container.diff.to_owned(),
        from_checkout: e.doc.from_checkout,
    }
    // PERF: converting the events into js values may hurt performance
    .into_js(doc);

    if let Err(e) = ob.call1(&event) {
        console_error!("Error when calling observer: {:#?}", e);
    }
}

#[allow(unused)]
fn call_after_micro_task(ob: observer::Observer, e: DiffEvent, doc: Arc<LoroDoc>) {
    let promise = Promise::resolve(&JsValue::NULL);
    type C = Closure<dyn FnMut(JsValue)>;
    let drop_handler: Rc<RefCell<Option<C>>> = Rc::new(RefCell::new(None));
    let copy = drop_handler.clone();
    let event = Event {
        id: e.doc.id(),
        from_children: e.from_children,
        from_checkout: e.doc.from_checkout,
        local: e.doc.local,
        origin: e.doc.origin.to_string(),
        target: e.container.id.clone(),
        diff: (e.container.diff.to_owned()),
        path: Event::get_path(
            e.container.path.len() as u32,
            e.container.path.iter().map(|x| &x.1),
        ),
    }
    .into_js(doc);

    let closure = Closure::once(move |_: JsValue| {
        let ans = ob.call1(&event);
        drop(copy);
        if let Err(e) = ans {
            console_error!("Error when calling observer: {:#?}", e);
        }
    });

    let _ = promise.then(&closure);
    drop_handler.borrow_mut().replace(closure);
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Event {
    pub local: bool,
    pub from_children: bool,
    id: u64,
    origin: String,
    target: ContainerID,
    from_checkout: bool,
    diff: Diff,
    path: JsValue,
}

impl Event {
    fn into_js(self, doc: Arc<LoroDoc>) -> JsValue {
        let obj = js_sys::Object::new();
        Reflect::set(&obj, &"local".into(), &self.local.into()).unwrap();
        Reflect::set(&obj, &"fromCheckout".into(), &self.from_checkout.into()).unwrap();
        Reflect::set(&obj, &"fromChildren".into(), &self.from_children.into()).unwrap();
        Reflect::set(&obj, &"origin".into(), &self.origin.into()).unwrap();
        Reflect::set(&obj, &"target".into(), &self.target.to_string().into()).unwrap();
        Reflect::set(&obj, &"diff".into(), &resolved_diff_to_js(self.diff, doc)).unwrap();
        Reflect::set(&obj, &"path".into(), &self.path).unwrap();
        Reflect::set(&obj, &"id".into(), &self.id.into()).unwrap();
        obj.into()
    }

    fn get_path<'a>(n: u32, source: impl Iterator<Item = &'a Index>) -> JsValue {
        let arr = Array::new_with_length(n);
        for (i, p) in source.enumerate() {
            arr.set(i as u32, p.clone().into());
        }
        let path: JsValue = arr.into_js_result().unwrap();
        path
    }
}

/// The handler of a text or richtext container.
#[wasm_bindgen]
pub struct LoroText {
    handler: TextHandler,
    _doc: Arc<LoroDoc>,
}

#[derive(Serialize, Deserialize)]
struct MarkRange {
    start: usize,
    end: usize,
}

#[wasm_bindgen]
impl LoroText {
    /// "Text"
    pub fn kind(&self) -> JsValue {
        JsValue::from_str("Text")
    }

    /// Insert some string at index.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello");
    /// ```
    pub fn insert(&mut self, index: usize, content: &str) -> JsResult<()> {
        debug_log::debug_log!("InsertLogWasm");
        self.handler.insert(index, content)?;
        Ok(())
    }

    /// Delete elements from index to index + len
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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

    /// Mark a range of text with a key and a value.
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
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// console.log(text.toDelta());  // [ { insert: 'Hello', attributes: { bold: true } } ]
    /// ```
    #[wasm_bindgen(js_name = "toDelta")]
    pub fn to_delta(&self) -> JsStringDelta {
        let delta = self.handler.get_richtext_value();
        let value: JsValue = delta.into();
        value.into()
    }

    /// Get the container id of the text.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.handler.id().into();
        value.into()
    }

    /// Get the length of text
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.handler.len_utf16()
    }

    /// Subscribe to the changes of the text.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let doc = loro.0.clone();
        let ans = loro.0.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, doc.clone());
            }),
        );

        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription.
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }

    /// Change the state of this text by delta.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const text = doc.getText("text");
    /// text.insert(0, "Hello World!");
    /// text.mark({ start: 0, end: 5 }, "bold", true);
    /// const delta = text.toDelta();
    /// const text2 = doc.getText("text2");
    /// text2.applyDelta(delta);
    /// ```
    #[wasm_bindgen(js_name = "applyDelta")]
    pub fn apply_delta(&self, delta: JsValue) -> JsResult<()> {
        let delta: Vec<TextDelta> = serde_wasm_bindgen::from_value(delta)?;
        console_log!("apply_delta {:?}", delta);
        self.handler.apply_delta(&delta)?;
        Ok(())
    }
}

/// The handler of a map container.
#[wasm_bindgen]
pub struct LoroMap {
    handler: MapHandler,
    doc: Arc<LoroDoc>,
}

const CONTAINER_TYPE_ERR: &str = "Invalid container type, only supports Text, Map, List, Tree";

#[wasm_bindgen]
impl LoroMap {
    /// "Map"
    pub fn kind(&self) -> JsValue {
        JsValue::from_str("Map")
    }

    /// Set the key with the value.
    ///
    /// If the value of the key is exist, the old value will be updated.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.set("foo", "baz");
    /// ```
    #[wasm_bindgen(js_name = "set")]
    pub fn insert(&mut self, key: &str, value: JsValue) -> JsResult<()> {
        self.handler.insert(key, value)?;
        Ok(())
    }

    /// Remove the key from the map.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const bar = map.get("foo");
    /// ```
    pub fn get(&self, key: &str) -> JsValueOrContainerOrUndefined {
        let v = self.handler.get_(key);
        (match v {
            Some(ValueOrContainer::Container(c)) => handler_to_js_value(c, self.doc.clone()),
            Some(ValueOrContainer::Value(v)) => v.into(),
            None => JsValue::UNDEFINED,
        })
        .into()
    }

    /// Get the keys of the map.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// map.set("baz", "bar");
    /// const values = map.values(); // ["bar", "bar"]
    /// ```
    pub fn values(&self) -> Vec<JsValue> {
        let mut ans: Vec<JsValue> = Vec::with_capacity(self.handler.len());
        self.handler.for_each(|_, v| {
            ans.push(loro_value_to_js_value_or_container(v, &self.doc));
        });
        ans
    }

    /// Get the entries of the map. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
            array.push(&loro_value_to_js_value_or_container(v, &self.doc));
            let v: JsValue = array.into();
            ans.push(v.into());
        });
        ans
    }

    /// The container id of this handler.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.handler.id().into();
        value.into()
    }

    /// Get the keys and the values. If the type of value is a child container,
    /// it will be resolved recursively.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const text = map.setContainer("text", "Text");
    /// text.insert(0, "Hello");
    /// console.log(map.getDeepValue());  // {"foo": "bar", "text": "Hello"}
    /// ```
    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> JsValue {
        self.handler.get_deep_value().into()
    }

    /// Set the key with a container.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// const text = map.setContainer("text", "Text");
    /// const list = map.setContainer("list", "List");
    /// ```
    #[wasm_bindgen(js_name = "setContainer")]
    pub fn insert_container(&mut self, key: &str, container_type: &str) -> JsResult<JsValue> {
        let type_ = match container_type {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            "tree" | "Tree" => ContainerType::Tree,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let c = self.handler.insert_container(key, type_)?;

        let container = match type_ {
            ContainerType::Map => LoroMap {
                handler: c.into_map().unwrap(),
                doc: self.doc.clone(),
            }
            .into(),
            ContainerType::List => LoroList {
                handler: c.into_list().unwrap(),
                doc: self.doc.clone(),
            }
            .into(),
            ContainerType::Text => LoroText {
                handler: c.into_text().unwrap(),
                _doc: self.doc.clone(),
            }
            .into(),
            ContainerType::Tree => LoroTree {
                handler: c.into_tree().unwrap(),
                doc: self.doc.clone(),
            }
            .into(),
        };
        Ok(container)
    }

    /// Subscribe to the changes of the map.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    ///
    /// @param {Listener} f - Event listener
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// map.set("foo", "bar");
    /// doc.commit();
    /// ```
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let doc = loro.0.clone();
        let id = loro.0.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, doc.clone());
            }),
        );

        Ok(id.into_u32())
    }

    /// Unsubscribe by the subscription.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// const subscription = map.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// map.set("foo", "bar");
    /// doc.commit();
    /// map.unsubscribe(doc, subscription);
    /// ```
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }

    /// Get the size of the map.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const map = doc.getMap("map");
    /// map.set("foo", "bar");
    /// console.log(map.size);   // 1
    /// ```
    #[wasm_bindgen(js_name = "size", method, getter)]
    pub fn size(&self) -> usize {
        self.handler.len()
    }
}

/// The handler of a list container.
#[wasm_bindgen]
pub struct LoroList {
    handler: ListHandler,
    doc: Arc<LoroDoc>,
}

#[wasm_bindgen]
impl LoroList {
    /// "List"
    pub fn kind(&self) -> JsValue {
        JsValue::from_str("List")
    }

    /// Insert a value at index.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// console.log(list.value);  // [100, "foo", true];
    /// ```
    pub fn insert(&mut self, index: usize, value: JsValue) -> JsResult<()> {
        self.handler.insert(index, value)?;
        Ok(())
    }

    /// Delete elements from index to index + len.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// console.log(list.get(0));  // 100
    /// console.log(list.get(1));  // undefined
    /// ```
    pub fn get(&self, index: usize) -> JsValueOrContainerOrUndefined {
        let Some(v) = self.handler.get_(index) else {
            return JsValue::UNDEFINED.into();
        };

        (match v {
            ValueOrContainer::Value(v) => v.into(),
            ValueOrContainer::Container(h) => handler_to_js_value(h, self.doc.clone()),
        })
        .into()
    }

    /// Get the id of this container.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.handler.id().into();
        value.into()
    }

    /// Get elements of the list. If the value is a child container, the corresponding
    /// `Container` will be returned.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// list.insert(1, "foo");
    /// list.insert(2, true);
    /// list.insertContainer(3, "Text");
    /// console.log(list.value);  // [100, "foo", true, LoroText];
    /// ```
    #[wasm_bindgen(js_name = "toArray", method)]
    pub fn to_array(&mut self) -> Vec<JsValueOrContainer> {
        let mut arr: Vec<JsValueOrContainer> = Vec::with_capacity(self.length());
        self.handler.for_each(|x| {
            arr.push(match x {
                ValueOrContainer::Value(v) => {
                    let v: JsValue = v.into();
                    v.into()
                }
                ValueOrContainer::Container(h) => {
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
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, "Text");
    /// text.insert(0, "Hello");
    /// console.log(list.getDeepValue());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> JsValue {
        let value = self.handler.get_deep_value();
        value.into()
    }

    /// Insert a container at the index.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.insert(0, 100);
    /// const text = list.insertContainer(1, "Text");
    /// text.insert(0, "Hello");
    /// console.log(list.getDeepValue());  // [100, "Hello"];
    /// ```
    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(&mut self, index: usize, container: &str) -> JsResult<JsValue> {
        let _type = match container {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            "tree" | "Tree" => ContainerType::Tree,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let c = self.handler.insert_container(index, _type)?;
        let container = match _type {
            ContainerType::Map => LoroMap {
                handler: c.into_map().unwrap(),
                doc: self.doc.clone(),
            }
            .into(),
            ContainerType::List => LoroList {
                handler: c.into_list().unwrap(),
                doc: self.doc.clone(),
            }
            .into(),
            ContainerType::Text => LoroText {
                handler: c.into_text().unwrap(),
                _doc: self.doc.clone(),
            }
            .into(),
            ContainerType::Tree => LoroTree {
                handler: c.into_tree().unwrap(),
                doc: self.doc.clone(),
            }
            .into(),
        };
        Ok(container)
    }

    /// Subscribe to the changes of the list.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// list.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// list.insert(0, 100);
    /// doc.commit();
    /// ```
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let doc = loro.0.clone();
        let ans = loro.0.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, doc.clone());
            }),
        );
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const list = doc.getList("list");
    /// const subscription = list.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// list.insert(0, 100);
    /// doc.commit();
    /// list.unsubscribe(doc, subscription);
    /// ```
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }

    /// Get the length of list.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
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
}

/// The handler of a tree(forest) container.
#[wasm_bindgen]
pub struct LoroTree {
    handler: TreeHandler,
    doc: Arc<LoroDoc>,
}

#[wasm_bindgen]
impl LoroTree {
    /// "Tree"
    pub fn kind(&self) -> JsValue {
        JsValue::from_str("Tree")
    }

    /// Create a new tree node as the child of parent and return an unique tree id.
    /// If the parent is undefined, the tree node will be a root node.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// /*
    /// [
    ///   {
    ///     id: '1@45D9F599E6B4209B',
    ///     parent: '0@45D9F599E6B4209B',
    ///     meta: 'cid:1@45D9F599E6B4209B:Map'
    ///   },
    ///   {
    ///     id: '0@45D9F599E6B4209B',
    ///     parent: null,
    ///     meta: 'cid:0@45D9F599E6B4209B:Map'
    ///   }
    /// ]
    ///  *\/
    /// console.log(tree.value);
    /// ```
    pub fn create(&mut self, parent: Option<JsTreeID>) -> JsResult<JsTreeID> {
        let id = if let Some(p) = parent {
            let parent: JsValue = p.into();
            let parent: TreeID = parent.try_into().unwrap_throw();
            self.handler.create(parent)?
        } else {
            self.handler.create(None)?
        };
        let js_id: JsValue = id.into();
        Ok(js_id.into())
    }

    /// Move the target tree node to be a child of the parent.
    /// It's not allowed that the target is an ancestor of the parent
    /// or the target and the parent are the same node.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// const node2 = tree.create(node);
    /// tree.mov(node2, root);
    /// // Error wiil be thrown if move operation creates a cycle
    /// tree.mov(root, node);
    /// ```
    pub fn mov(&mut self, target: JsTreeID, parent: Option<JsTreeID>) -> JsResult<()> {
        let target: JsValue = target.into();
        let target = TreeID::try_from(target).unwrap();
        let parent = if let Some(parent) = parent {
            let parent: JsValue = parent.into();
            let parent = TreeID::try_from(parent).unwrap();
            Some(parent)
        } else {
            None
        };
        self.handler.mov(target, parent)?;
        Ok(())
    }

    /// Delete a tree node from the forest.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// tree.delete(node);
    /// /*
    /// [
    ///   {
    ///     id: '0@40553779E43298C6',
    ///     parent: null,
    //     meta: 'cid:0@40553779E43298C6:Map'
    ///   }
    /// ]
    ///  *\/
    /// console.log(tree.value);
    /// ```
    pub fn delete(&mut self, target: JsTreeID) -> JsResult<()> {
        let target: JsValue = target.into();
        self.handler.delete(target.try_into().unwrap())?;
        Ok(())
    }

    /// Get the associated metadata map container of a tree node.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const rootMeta = tree.getMeta(root);
    /// rootMeta.set("color", "red");
    /// // [ { id: '0@F2462C4159C4C8D1', parent: null, meta: { color: 'red' } } ]
    /// console.log(tree.getDeepValue());
    /// ```
    #[wasm_bindgen(js_name = "getMeta")]
    pub fn get_meta(&mut self, target: JsTreeID) -> JsResult<LoroMap> {
        let target: JsValue = target.into();
        let meta = self.handler.get_meta(target.try_into().unwrap())?;
        Ok(LoroMap {
            handler: meta,
            doc: self.doc.clone(),
        })
    }

    /// Get the id of the container.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.handler.id().into();
        value.into()
    }

    /// Return `true` if the tree contains the TreeID, `false` if the target is deleted or wrong.
    #[wasm_bindgen(js_name = "has")]
    pub fn contains(&self, target: JsTreeID) -> bool {
        let target: JsValue = target.into();
        self.handler.contains(target.try_into().unwrap())
    }

    /// Get the flat array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use `getDeepValue`.
    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.handler.get_value().into()
    }

    /// Get the flat array with metadata of the forest.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const rootMeta = tree.getMeta(root);
    /// rootMeta.set("color", "red");
    /// // [ { id: '0@F2462C4159C4C8D1', parent: null, meta: 'cid:0@F2462C4159C4C8D1:Map' } ]
    /// console.log(tree.value);
    /// // [ { id: '0@F2462C4159C4C8D1', parent: null, meta: { color: 'red' } } ]
    /// console.log(tree.getDeepValue());
    /// ```
    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> JsValue {
        self.handler.get_deep_value().into()
    }

    /// Get all tree ids of the forest.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// const node2 = tree.create(node);
    /// console.log(tree.nodes) // [ '1@A5024AE0E00529D2', '2@A5024AE0E00529D2', '0@A5024AE0E00529D2' ]
    /// ```
    #[wasm_bindgen(js_name = "nodes", method, getter)]
    pub fn nodes(&mut self) -> Vec<JsTreeID> {
        self.handler
            .nodes()
            .into_iter()
            .map(|n| {
                let v: JsValue = n.into();
                v.into()
            })
            .collect()
    }

    /// Get the parent of the specific node.
    /// Return undefined if the target is a root node.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// const node2 = tree.create(node);
    /// console.log(tree.parent(node2)) // '1@B75DEC6222870A0'
    /// console.log(tree.parent(root))  // undefined
    /// ```
    pub fn parent(&mut self, target: JsTreeID) -> JsResult<Option<JsTreeID>> {
        let target: JsValue = target.into();
        let id = target
            .try_into()
            .map_err(|_| LoroError::JsError("parse `TreeID` string error".into()))?;
        self.handler
            .parent(id)
            .map(|p| {
                p.map(|p| {
                    let v: JsValue = p.into();
                    v.into()
                })
            })
            .ok_or(format!("Tree node `{}` doesn't exist", id).into())
    }

    /// Subscribe to the changes of the tree.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// tree.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// doc.commit();
    /// ```
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let doc = loro.0.clone();
        let ans = loro.0.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e, doc.clone());
            }),
        );
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription.
    ///
    /// @example
    /// ```ts
    /// import { Loro } from "loro-crdt";
    ///
    /// const doc = new Loro();
    /// const tree = doc.getTree("tree");
    /// const subscription = tree.subscribe((event)=>{
    ///     console.log(event);
    /// });
    /// const root = tree.create();
    /// const node = tree.create(root);
    /// doc.commit();
    /// tree.unsubscribe(doc, subscription);
    /// ```
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }
}

/// Convert a encoded version vector to a readable js Map.
///
/// @example
/// ```ts
/// import { Loro } from "loro-crdt";
///
/// const doc = new Loro();
/// doc.setPeerId('100');
/// doc.getText("t").insert(0, 'a');
/// doc.commit();
/// const version = doc.getVersion();
/// const readableVersion = convertVersionToReadableObj(version);
/// console.log(readableVersion); // Map(1) { 100n => 1 }
/// ```
#[wasm_bindgen(js_name = "toReadableVersion")]
pub fn to_readable_version(version: &[u8]) -> Result<JsVersionVectorMap, JsValue> {
    let version_vector = VersionVector::decode(version)?;
    let map = vv_to_js_value(version_vector);
    Ok(JsVersionVectorMap::from(map))
}

/// Convert a readable js Map to a encoded version vector.
///
/// @example
/// ```ts
/// import { Loro } from "loro-crdt";
///
/// const doc = new Loro();
/// doc.setPeerId('100');
/// doc.getText("t").insert(0, 'a');
/// doc.commit();
/// const version = doc.getVersion();
/// const readableVersion = convertVersionToReadableObj(version);
/// console.log(readableVersion); // Map(1) { 100n => 1 }
/// const encodedVersion = toEncodedVersion(readableVersion);
/// ```
#[wasm_bindgen(js_name = "toEncodedVersion")]
pub fn to_encoded_version(version: JsVersionVectorMap) -> Result<Vec<u8>, JsValue> {
    let map: JsValue = version.into();
    let map: js_sys::Map = map.into();
    let vv = js_map_to_vv(map)?;
    let encoded = vv.encode();
    Ok(encoded)
}

fn vv_to_js_value(vv: VersionVector) -> JsValue {
    let map = js_sys::Map::new();
    for (k, v) in vv.iter() {
        let k = k.to_string().into();
        let v = JsValue::from(*v);
        map.set(&k, &v);
    }

    map.into()
}

fn loro_value_to_js_value_or_container(value: ValueOrContainer, doc: &Arc<LoroDoc>) -> JsValue {
    match value {
        ValueOrContainer::Value(v) => {
            let value: JsValue = v.into();
            value
        }
        ValueOrContainer::Container(c) => {
            let handler: JsValue = handler_to_js_value(c, doc.clone());
            handler
        }
    }
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
/**
* Container types supported by loro.
*
* It is most commonly used to specify the type of subcontainer to be created.
* @example
* ```ts
* import { Loro } from "loro-crdt";
*
* const doc = new Loro();
* const list = doc.getList("list");
* list.insert(0, 100);
* const containerType = "Text";
* const text = list.insertContainer(1, containerType);
* ```
*/
export type ContainerType = "Text" | "Map" | "List"| "Tree";
export type PeerID = string;
/**
* The unique id of each container.
*
* @example
* ```ts
* import { Loro } from "loro-crdt";
*
* const doc = new Loro();
* const list = doc.getList("list");
* const containerId = list.id;
* ```
*/
export type ContainerID =
  | `cid:root-${string}:${ContainerType}`
  | `cid:${number}@${string}:${ContainerType}`;

/**
 * The unique id of each tree node.
 */
export type TreeID = `${number}@${string}`;

interface Loro {
    exportFrom(version?: Uint8Array): Uint8Array;
    exportFromV0(version?: Uint8Array): Uint8Array;
    exportFromCompressed(version?: Uint8Array): Uint8Array;
    getContainerById(id: ContainerID): LoroText | LoroMap | LoroList;
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

export type Container = LoroList | LoroMap | LoroText | LoroTree;
"#;
