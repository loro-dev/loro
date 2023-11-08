use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use loro_internal::{
    change::{Lamport, Timestamp},
    container::{
        richtext::{ExpandType, TextStyleInfoFlag},
        ContainerID,
    },
    event::{Diff, Index},
    handler::{ListHandler, MapHandler, TextDelta, TextHandler, TreeHandler},
    id::{Counter, PeerID, TreeID, ID},
    obs::SubID,
    version::Frontiers,
    ContainerType, DiffEvent, LoroDoc, LoroError, LoroValue, VersionVector,
};
use rle::HasLength;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, cmp::Ordering, ops::Deref, panic, rc::Rc, sync::Arc};
use wasm_bindgen::{__rt::IntoJsResult, prelude::*};
mod log;
mod prelim;
pub use prelim::{PrelimList, PrelimMap, PrelimText};

mod convert;

#[wasm_bindgen(js_name = setPanicHook)]
pub fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[wasm_bindgen(js_name = setDebug)]
pub fn set_debug(filter: &str) {
    debug_log::set_debug(filter)
}

type JsResult<T> = Result<T, JsValue>;

/// The CRDT document.
///
/// When FinalizationRegistry is unavailable, it's the users' responsibility to free the document.
#[wasm_bindgen]
pub struct Loro(LoroDoc);

impl Deref for Loro {
    type Target = LoroDoc;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
    #[wasm_bindgen(typescript_type = "{ peer: bigint, counter: number }")]
    pub type JsID;
    #[wasm_bindgen(
        typescript_type = "{ start: number, end: number, expand?: 'before'|'after'|'both'|'none' }"
    )]
    pub type JsRange;
    #[wasm_bindgen(typescript_type = "number|bool|string|null")]
    pub type JsMarkValue;
    #[wasm_bindgen(typescript_type = "TreeID")]
    pub type JsTreeID;
    #[wasm_bindgen(typescript_type = "Delta<string>[]")]
    pub type JsStringDelta;
    #[wasm_bindgen(typescript_type = "Map<bigint, number>")]
    pub type JsVersionVectorMap;
    #[wasm_bindgen(typescript_type = "Map<BigInt, Change[]>")]
    pub type JsChanges;
    #[wasm_bindgen(typescript_type = "Change")]
    pub type JsChange;
    #[wasm_bindgen(typescript_type = "Map<bigint, number> | Uint8Array")]
    pub type JsVersionVector;
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
    let peer: u64 = Reflect::get(&id, &"peer".into())?.try_into()?;
    let counter = Reflect::get(&id, &"counter".into())?.as_f64().unwrap() as Counter;
    let id = ID::new(peer, counter);
    Ok(id)
}

fn frontiers_to_ids(frontiers: &Frontiers) -> Vec<JsID> {
    let mut ans = Vec::with_capacity(frontiers.len());
    for id in frontiers.iter() {
        let obj = Object::new();
        Reflect::set(&obj, &"peer".into(), &id.peer.into()).unwrap();
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
struct ChangeMeta {
    lamport: Lamport,
    length: usize,
    peer: PeerID,
    counter: Counter,
    deps: Vec<ID>,
    timestamp: Timestamp,
}

#[wasm_bindgen]
impl Loro {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut doc = LoroDoc::new();
        doc.start_auto_commit();
        Self(doc)
    }

    #[wasm_bindgen(js_name = "fromSnapshot")]
    pub fn from_snapshot(snapshot: &[u8]) -> JsResult<Loro> {
        let doc = LoroDoc::from_snapshot(snapshot)?;
        Ok(Loro(doc))
    }

    /// Attach the document state to the latest known version.
    ///
    /// > The document becomes detached during a `checkout` operation.
    /// > Being `detached` implies that the `DocState` is not synchronized with the latest version of the `OpLog`.
    /// > In a detached state, the document is not editable, and any `import` operations will be
    /// > recorded in the `OpLog` without being applied to the `DocState`.
    ///
    /// This method has the same effect as invoking `checkout_to_latest`.
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
    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&self, cid: &JsIntoContainerID) -> JsResult<LoroText> {
        let text = self
            .0
            .get_text(js_value_to_container_id(cid, ContainerType::Text)?);
        Ok(LoroText(text))
    }

    /// Get a LoroMap by container id
    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&self, cid: &JsIntoContainerID) -> JsResult<LoroMap> {
        let map = self
            .0
            .get_map(js_value_to_container_id(cid, ContainerType::Map)?);
        Ok(LoroMap(map))
    }

    /// Get a LoroList by container id
    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&self, cid: &JsIntoContainerID) -> JsResult<LoroList> {
        let list = self
            .0
            .get_list(js_value_to_container_id(cid, ContainerType::List)?);
        Ok(LoroList(list))
    }

    /// Get a LoroTree by container id
    #[wasm_bindgen(js_name = "getTree")]
    pub fn get_tree(&self, cid: &JsIntoContainerID) -> JsResult<LoroTree> {
        let tree = self
            .0
            .get_tree(js_value_to_container_id(cid, ContainerType::Tree)?);
        Ok(LoroTree(tree))
    }

    /// Get the container corresponding to the container id
    #[wasm_bindgen(skip_typescript, js_name = "getContainerById")]
    pub fn get_container_by_id(&self, container_id: JsContainerID) -> JsResult<JsValue> {
        let container_id: ContainerID = container_id.to_owned().try_into()?;
        let ty = container_id.container_type();
        Ok(match ty {
            ContainerType::Map => {
                let map = self.0.get_map(container_id);
                LoroMap(map).into()
            }
            ContainerType::List => {
                let list = self.0.get_list(container_id);
                LoroList(list).into()
            }
            ContainerType::Text => {
                let richtext = self.0.get_text(container_id);
                LoroText(richtext).into()
            }
            ContainerType::Tree => {
                let tree = self.0.get_tree(container_id);
                LoroTree(tree).into()
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

    /// Get the encoded version vector of the lastest verison in OpLog.
    ///
    /// If you checkout to a specific version, the version vector will not change.
    #[inline(always)]
    pub fn oplog_version(&self) -> Vec<u8> {
        self.0.state_vv().encode()
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
    #[wasm_bindgen(skip_typescript, js_name = "exportFrom")]
    pub fn export_from(&self, version: &JsValue) -> JsResult<Vec<u8>> {
        // `version` may be null or undefined
        let vv = js_value_to_version(version)?;
        Ok(self.0.export_from(&vv))
    }

    /// Import a snapshot or a update
    pub fn import(&self, update_or_snapshot: &[u8]) -> JsResult<()> {
        self.0.import(update_or_snapshot)?;
        Ok(())
    }

    /// Import a batch of updates.
    ///
    /// It's more efficient than importing updates one by one.
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

    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> JsResult<JsValue> {
        let json = self.0.get_deep_value();
        Ok(json.into())
    }

    /// Subscribe to the changes of the loro document. The function will be called when the
    /// transaction is committed or updates from remote are imported.
    ///
    /// Returns a subscription ID, which can be used to unsubscribe.
    // TODO: convert event and event sub config
    pub fn subscribe(&self, f: js_sys::Function) -> u32 {
        let observer = observer::Observer::new(f);
        self.0
            .subscribe_root(Arc::new(move |e| {
                // call_after_micro_task(observer.clone(), e)
                call_subscriber(observer.clone(), e);
            }))
            .into_u32()
    }

    /// Unsubscribe by the subscription
    pub fn unsubscribe(&self, subscription: u32) {
        self.0.unsubscribe(SubID::from_u32(subscription))
    }

    /// Debug the size of the history
    #[wasm_bindgen(js_name = "debugHistory")]
    pub fn debug_history(&self) {
        let oplog = self.0.oplog().lock().unwrap();
        console_log!("{:#?}", oplog.diagnose_size());
    }

    /// Get all of changes in the oplog
    #[wasm_bindgen(js_name = "getAllChanges")]
    pub fn get_all_changes(&self) -> JsChanges {
        let oplog = self.0.oplog().lock().unwrap();
        let changes = oplog.changes();
        let ans = js_sys::Map::new();
        for (peer_id, changes) in changes {
            let row = js_sys::Array::new_with_length(changes.len() as u32);
            for (i, change) in changes.iter().enumerate() {
                let change = ChangeMeta {
                    lamport: change.lamport,
                    length: change.atom_len(),
                    peer: change.peer(),
                    counter: change.id.counter,
                    deps: change.deps.iter().cloned().collect(),
                    timestamp: change.timestamp,
                };
                row.set(i as u32, serde_wasm_bindgen::to_value(&change).unwrap());
            }
            ans.set(&js_sys::BigInt::from(*peer_id).into(), &row);
        }

        let value: JsValue = ans.into();
        value.into()
    }

    #[wasm_bindgen(js_name = "getChangeAt")]
    pub fn get_change_at(&self, id: JsID) -> JsResult<JsChange> {
        let id = js_id_to_id(id)?;
        let oplog = self.0.oplog().lock().unwrap();
        let change = oplog
            .get_change_at(id)
            .ok_or_else(|| JsError::new(&format!("Change {:?} not found", id)))?;
        let change = ChangeMeta {
            lamport: change.lamport,
            length: change.atom_len(),
            peer: change.peer(),
            counter: change.id.counter,
            deps: change.deps.iter().cloned().collect(),
            timestamp: change.timestamp,
        };
        Ok(serde_wasm_bindgen::to_value(&change).unwrap().into())
    }

    #[wasm_bindgen(js_name = "getOpsInChange")]
    pub fn get_ops_in_change(&self, id: JsID) -> JsResult<Vec<JsValue>> {
        let id = js_id_to_id(id)?;
        let oplog = self.0.oplog().lock().unwrap();
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
    #[wasm_bindgen(js_name = "frontiersToVV")]
    pub fn frontiers_to_vv(&self, frontiers: Vec<JsID>) -> JsResult<JsVersionVectorMap> {
        let frontiers = ids_to_frontiers(frontiers)?;
        let oplog = self.0.oplog().try_lock().unwrap();
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
    #[wasm_bindgen(js_name = "vvToFrontiers")]
    pub fn vv_to_frontiers(&self, vv: &JsVersionVector) -> JsResult<Vec<JsID>> {
        let value: JsValue = vv.into();
        let is_bytes = value.is_instance_of::<js_sys::Uint8Array>();
        let vv = if is_bytes {
            let bytes = js_sys::Uint8Array::try_from(value.clone()).unwrap_throw();
            let bytes = bytes.to_vec();
            VersionVector::decode(&bytes)?
        } else {
            let map = js_sys::Map::try_from(value).unwrap_throw();
            js_map_to_vv(map)?
        };

        let f = self.0.oplog().lock().unwrap().dag().vv_to_frontiers(&vv);
        Ok(frontiers_to_ids(&f))
    }

    #[wasm_bindgen(js_name = "getDeepValue")]
    pub fn get_deep_value(&self) -> JsValue {
        let value = self.0.get_deep_value();
        JsValue::from(value)
    }
}

fn js_map_to_vv(map: js_sys::Map) -> JsResult<VersionVector> {
    let mut vv = VersionVector::new();
    for pair in map.entries() {
        let pair = pair.unwrap_throw();
        let key = Reflect::get(&pair, &0.into()).unwrap_throw();
        let peer_id = u64::try_from(key.clone()).expect_throw("PeerID must be u64");
        let value = Reflect::get(&pair, &1.into()).unwrap_throw();
        let counter = value.as_f64().expect_throw("Invalid counter") as Counter;
        vv.insert(peer_id, counter);
    }

    Ok(vv)
}

#[allow(unused)]
fn call_subscriber(ob: observer::Observer, e: DiffEvent) {
    // We convert the event to js object here, so that we don't need to worry about GC.
    // In the future, when FinalizationRegistry[1] is stable, we can use `--weak-ref`[2] feature
    // in wasm-bindgen to avoid this.
    //
    // [1]: https://caniuse.com/?search=FinalizationRegistry
    // [2]: https://rustwasm.github.io/wasm-bindgen/reference/weak-references.html
    let event = Event {
        path: Event::get_path(
            e.container.path.len() as u32,
            e.container.path.iter().map(|x| &x.1),
        ),
        from_children: e.from_children,
        local: e.doc.local,
        origin: e.doc.origin.to_string(),
        target: e.container.id.clone(),
        diff: e.container.diff.to_owned(),
    }
    // PERF: converting the events into js values may hurt performance
    .into_js();

    if let Err(e) = ob.call1(&event) {
        console_error!("Error when calling observer: {:#?}", e);
    }
}

#[allow(unused)]
fn call_after_micro_task(ob: observer::Observer, e: DiffEvent) {
    let promise = Promise::resolve(&JsValue::NULL);
    type C = Closure<dyn FnMut(JsValue)>;
    let drop_handler: Rc<RefCell<Option<C>>> = Rc::new(RefCell::new(None));
    let copy = drop_handler.clone();
    let event = Event {
        from_children: e.from_children,
        local: e.doc.local,
        origin: e.doc.origin.to_string(),
        target: e.container.id.clone(),
        diff: (e.container.diff.to_owned()),
        path: Event::get_path(
            e.container.path.len() as u32,
            e.container.path.iter().map(|x| &x.1),
        ),
    }
    .into_js();

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
    origin: String,
    target: ContainerID,
    diff: Diff,
    path: JsValue,
}

impl Event {
    fn into_js(self) -> JsValue {
        let obj = js_sys::Object::new();
        Reflect::set(&obj, &"local".into(), &self.local.into()).unwrap();
        Reflect::set(&obj, &"fromChildren".into(), &self.from_children.into()).unwrap();
        Reflect::set(&obj, &"origin".into(), &self.origin.into()).unwrap();
        Reflect::set(&obj, &"target".into(), &self.target.to_string().into()).unwrap();
        Reflect::set(&obj, &"diff".into(), &self.diff.into()).unwrap();
        Reflect::set(&obj, &"path".into(), &self.path).unwrap();
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
pub struct LoroText(TextHandler);

#[derive(Serialize, Deserialize)]
struct MarkRange {
    start: usize,
    end: usize,
    expand: Option<String>,
}

#[wasm_bindgen]
impl LoroText {
    /// Insert some string at index.
    pub fn insert(&mut self, index: usize, content: &str) -> JsResult<()> {
        debug_log::debug_log!("InsertLogWasm");
        self.0.insert_(index, content)?;
        Ok(())
    }

    /// Delete elements from index to index + len
    pub fn delete(&mut self, index: usize, len: usize) -> JsResult<()> {
        self.0.delete_(index, len)?;
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
    pub fn mark(&self, range: JsRange, key: &str, value: JsValue) -> Result<(), JsError> {
        let range: MarkRange = serde_wasm_bindgen::from_value(range.into())?;
        let value: LoroValue = LoroValue::try_from(value)?;
        let expand = range
            .expand
            .map(|x| {
                ExpandType::try_from_str(&x)
                    .expect_throw("`expand` must be one of `none`, `start`, `end`, `both`")
            })
            .unwrap_or(ExpandType::After);
        self.0.mark_(
            range.start,
            range.end,
            key,
            value,
            TextStyleInfoFlag::new(true, expand, false, false),
        )?;
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
    pub fn unmark(&self, range: JsRange, key: &str) -> Result<(), JsValue> {
        // Internally, this may be marking with null or deleting all the marks with key in the range entirely.
        let range: MarkRange = serde_wasm_bindgen::from_value(range.into())?;
        let expand = range
            .expand
            .map(|x| {
                ExpandType::try_from_str(&x)
                    .expect_throw("`expand` must be one of `none`, `start`, `end`, `both`")
            })
            .unwrap_or(ExpandType::After);
        let expand = expand.reverse();
        self.0.mark_(
            range.start,
            range.end,
            key,
            LoroValue::Null,
            TextStyleInfoFlag::new(true, expand, false, false),
        )?;
        Ok(())
    }

    /// Convert the state to string
    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.0.get_value().as_string().unwrap().to_string()
    }

    /// Get the text in [Delta](https://quilljs.com/docs/delta/) format.
    ///
    /// The returned value will include the rich text information.
    #[wasm_bindgen(js_name = "toDelta")]
    pub fn to_delta(&self) -> JsStringDelta {
        let delta = self.0.get_richtext_value();
        let value: JsValue = delta.into();
        value.into()
    }

    /// Get the container id of the text.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    /// Get the length of text
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.0.len_utf16()
    }

    /// Subscribe to the changes of the text.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let ans = loro.0.subscribe(
            &self.0.id(),
            Arc::new(move |e| {
                call_subscriber(observer.clone(), e);
            }),
        );

        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription.
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }

    /// Change the state of this text by delta format.
    #[wasm_bindgen(js_name = "applyDelta")]
    pub fn apply_delta(&self, delta: JsValue) -> JsResult<()> {
        let delta: Vec<TextDelta> = serde_wasm_bindgen::from_value(delta)?;
        console_log!("apply_delta {:?}", delta);
        self.0.apply_delta_(&delta)?;
        Ok(())
    }
}

/// The handler of a map container.
#[wasm_bindgen]
pub struct LoroMap(MapHandler);
const CONTAINER_TYPE_ERR: &str = "Invalid container type, only supports Text, Map, List, Tree";

#[wasm_bindgen]
impl LoroMap {
    /// Set the key with the value.
    ///
    /// If the value of the key is exist, the old value will be updated.
    #[wasm_bindgen(js_name = "set")]
    pub fn insert(&mut self, key: &str, value: JsValue) -> JsResult<()> {
        self.0.insert_(key, value.into())?;
        Ok(())
    }

    /// Remove the key from the map.
    pub fn delete(&mut self, key: &str) -> JsResult<()> {
        self.0.delete_(key)?;
        Ok(())
    }

    /// Get the value of the key.
    pub fn get(&self, key: &str) -> JsValue {
        self.0.get(key).into()
    }

    /// Get the keys and values.
    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> JsValue {
        let value = self.0.get_value();
        value.into()
    }

    /// The container id of this handler.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    /// Get the keys and the values. If the type of value is a container, it will be
    /// resolved recursively.
    #[wasm_bindgen(js_name = "getDeepValue")]
    pub fn get_value_deep(&self) -> JsValue {
        self.0.get_deep_value().into()
    }

    /// Set the key with a container.
    #[wasm_bindgen(js_name = "setContainer")]
    pub fn insert_container(&mut self, key: &str, container_type: &str) -> JsResult<JsValue> {
        let type_ = match container_type {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            "tree" | "Tree" => ContainerType::Tree,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let c = self.0.insert_container_(key, type_)?;

        let container = match type_ {
            ContainerType::Map => LoroMap(c.into_map().unwrap()).into(),
            ContainerType::List => LoroList(c.into_list().unwrap()).into(),
            ContainerType::Tree => LoroTree(c.into_tree().unwrap()).into(),
            ContainerType::Text => LoroText(c.into_text().unwrap()).into(),
        };
        Ok(container)
    }

    /// Subscribe to the changes of the map.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let id = loro.0.subscribe(
            &self.0.id(),
            Arc::new(move |e| {
                call_subscriber(observer.clone(), e);
            }),
        );

        Ok(id.into_u32())
    }

    /// Unsubscribe by the subscription.
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }

    /// Get the size of the map.
    #[wasm_bindgen(js_name = "size", method, getter)]
    pub fn size(&self) -> usize {
        self.0.len()
    }
}

/// The handler of a list container.
#[wasm_bindgen]
pub struct LoroList(ListHandler);

#[wasm_bindgen]
impl LoroList {
    /// Insert a value at index.
    pub fn insert(&mut self, index: usize, value: JsValue) -> JsResult<()> {
        self.0.insert_(index, value.into())?;
        Ok(())
    }

    /// Delete elements from index to index + len.
    pub fn delete(&mut self, index: usize, len: usize) -> JsResult<()> {
        self.0.delete_(index, len)?;
        Ok(())
    }

    /// Get the value at the index.
    pub fn get(&self, index: usize) -> JsValue {
        let Some(v) = self.0.get(index) else {
            return JsValue::UNDEFINED;
        };

        JsValue::from(v)
    }

    /// Get the id of this container.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    /// Get elements of the list.
    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.0.get_value().into()
    }

    /// Get elements of the list. If the type of a element is a container, it will be
    /// resolved recursively.
    #[wasm_bindgen(js_name = "getDeepValue")]
    pub fn get_deep_value(&self) -> JsValue {
        let value = self.0.get_deep_value();
        value.into()
    }

    /// Insert a container at the index.
    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(&mut self, index: usize, container: &str) -> JsResult<JsValue> {
        let _type = match container {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            "tree" | "Tree" => ContainerType::Tree,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let c = self.0.insert_container_(index, _type)?;
        let container = match _type {
            ContainerType::Map => LoroMap(c.into_map().unwrap()).into(),
            ContainerType::List => LoroList(c.into_list().unwrap()).into(),
            ContainerType::Text => LoroText(c.into_text().unwrap()).into(),
            ContainerType::Tree => LoroTree(c.into_tree().unwrap()).into(),
        };
        Ok(container)
    }

    /// Subscribe to the changes of the list.
    ///
    /// returns a subscription id, which can be used to unsubscribe.
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let ans = loro.0.subscribe(
            &self.0.id(),
            Arc::new(move |e| {
                call_subscriber(observer.clone(), e);
            }),
        );
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription.
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }

    /// Get the length of list.
    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.0.len()
    }
}

/// The handler of a tree(forest) container.
#[wasm_bindgen]
pub struct LoroTree(TreeHandler);

#[wasm_bindgen]
impl LoroTree {
    /// Create a new tree node as the child of parent and return an unique tree id.
    /// If the parent is undefined, the tree node will be a root node.
    pub fn create(&mut self, parent: Option<JsTreeID>) -> JsResult<JsTreeID> {
        let id = if let Some(p) = parent {
            let parent: JsValue = p.into();
            self.0.create_and_mov_(parent.try_into().unwrap_throw())?
        } else {
            self.0.create_()?
        };
        let js_id: JsValue = id.into();
        Ok(js_id.into())
    }

    /// Move the target tree node to be a child of the parent.
    /// It's not allowed that the target is an ancestor of the parent
    /// or the target and the parent are the same node.
    pub fn mov(&mut self, target: JsTreeID, parent: JsTreeID) -> JsResult<()> {
        let target: JsValue = target.into();
        let target = TreeID::try_from(target).unwrap();
        let parent: JsValue = parent.into();
        let parent = TreeID::try_from(parent).unwrap();
        self.0.mov_(target, parent)?;
        Ok(())
    }

    /// Delete a tree node from the forest.
    pub fn delete(&mut self, target: JsTreeID) -> JsResult<()> {
        let target: JsValue = target.into();
        self.0.delete_(target.try_into().unwrap())?;
        Ok(())
    }

    /// Set the tree node as root.
    pub fn root(&mut self, target: JsTreeID) -> JsResult<()> {
        let target: JsValue = target.into();
        self.0.as_root_(target.try_into().unwrap())?;
        Ok(())
    }

    /// Get the associated metadata map container of a tree node.
    #[wasm_bindgen(js_name = "getMeta")]
    pub fn get_meta(&mut self, target: JsTreeID) -> JsResult<LoroMap> {
        let target: JsValue = target.into();
        let meta = self.0.get_meta(target.try_into().unwrap())?;
        Ok(LoroMap(meta))
    }

    /// Get the id of the container.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    /// Get the flat array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metatdata, you should use `getDeepValue`.
    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.0.get_value().into()
    }

    /// Get the flat array with metadata of the forest.
    #[wasm_bindgen(js_name = "getDeepValue")]
    pub fn get_value_deep(&self) -> JsValue {
        self.0.get_deep_value().into()
    }

    /// Get all tree ids of the forest.
    #[wasm_bindgen(js_name = "nodes", method, getter)]
    pub fn nodes(&mut self) -> Vec<JsTreeID> {
        self.0
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
    #[wasm_bindgen(js_name = "parent")]
    pub fn parent(&mut self, target: JsTreeID) -> JsResult<Option<JsTreeID>> {
        let target: JsValue = target.into();
        let id = target
            .try_into()
            .map_err(|_| LoroError::JsError("parse `TreeID` string error".into()))?;
        self.0
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
    pub fn subscribe(&self, loro: &Loro, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let ans = loro.0.subscribe(
            &self.0.id(),
            Arc::new(move |e| {
                call_subscriber(observer.clone(), e);
            }),
        );
        Ok(ans.into_u32())
    }

    /// Unsubscribe by the subscription.
    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }
}

/// Convert a encoded version vector to a readable js Map.
///
/// # Example
///
/// ```js
/// const loro = new Loro();
/// loro.setPeerId('100');
/// loro.getText("t").insert(0, 'a');
/// loro.commit();
/// const version = loro.getVersion();
/// const readableVersion = convertVersionToReadableObj(version);
/// console.log(readableVersion); // Map(1) { 100n => 1 }
/// ```
#[wasm_bindgen(js_name = "toReadableVersion")]
pub fn to_readable_version(version: &[u8]) -> Result<JsVersionVectorMap, JsValue> {
    let version_vector = VersionVector::decode(version)?;
    let map = vv_to_js_value(version_vector);
    Ok(JsVersionVectorMap::from(map))
}

#[wasm_bindgen(js_name = "toEncodedVersion")]
pub fn to_encoded_version(version: JsVersionVectorMap) -> Result<Vec<u8>, JsValue> {
    let map: JsValue = version.into();
    let map: js_sys::Map = map.try_into().unwrap_throw();
    let vv = js_map_to_vv(map)?;
    let encoded = vv.encode();
    Ok(encoded)
}

fn vv_to_js_value(vv: VersionVector) -> JsValue {
    let map = js_sys::Map::new();
    for (k, v) in vv.iter() {
        let k = js_sys::BigInt::from(*k);
        let v = JsValue::from(*v);
        map.set(&k.to_owned(), &v);
    }

    map.into()
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
export type ContainerType = "Text" | "Map" | "List"| "Tree";
export type ContainerID =
  | `cid:root-${string}:${ContainerType}`
  | `cid:${number}@${string}:${ContainerType}`;
export type TreeID = `${number}@${string}`;

interface Loro {
    exportFrom(version?: Uint8Array): Uint8Array;
    getContainerById(id: ContainerID): LoroText | LoroMap | LoroList;
}
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

export type OpId = { peer: bigint, counter: number };
/**
 * Change is a group of continuous operations
 */
export interface Change {
    peer: BigInt,
    counter: number,
    lamport: number,
    length: number,
    deps: OpId[],
}
"#;
