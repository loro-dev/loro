use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use loro_internal::{
    configure::SecureRandomGenerator,
    container::ContainerID,
    event::{Diff, Index},
    handler::{ListHandler, MapHandler, TextHandler},
    id::{Counter, ID},
    obs::SubID,
    txn::Transaction as Txn,
    version::Frontiers,
    ContainerType, DiffEvent, LoroDoc, VersionVector,
};
use std::{cell::RefCell, cmp::Ordering, ops::Deref, rc::Rc, sync::Arc};
use wasm_bindgen::{__rt::IntoJsResult, prelude::*};
mod log;
mod prelim;
pub use prelim::{PrelimList, PrelimMap, PrelimText};

mod convert;

#[wasm_bindgen(js_name = setPanicHook)]
pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function at least once during initialization, and then
    // we will get better error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
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
    #[wasm_bindgen(typescript_type = "Transaction | Loro")]
    pub type JsTransaction;
    #[wasm_bindgen(typescript_type = "string | undefined")]
    pub type JsOrigin;
    #[wasm_bindgen(typescript_type = "{ peer: bigint, counter: number }")]
    pub type JsID;
}

struct MathRandom;
impl SecureRandomGenerator for MathRandom {
    fn fill_byte(&self, dest: &mut [u8]) {
        let mut bytes: [u8; 8] = js_sys::Math::random().to_be_bytes();
        let mut index = 0;
        let mut count = 0;
        while index < dest.len() {
            dest[index] = bytes[count];
            index += 1;
            count += 1;
            if count == 8 {
                bytes = js_sys::Math::random().to_be_bytes();
                count = 0;
            }
        }
    }
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
        let peer: u64 = Reflect::get(&id, &"peer".into())?.try_into()?;
        let counter = Reflect::get(&id, &"counter".into())?.as_f64().unwrap() as Counter;
        frontiers.push(ID::new(peer, counter));
    }

    Ok(frontiers)
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

#[wasm_bindgen]
impl Loro {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(LoroDoc::new())
    }

    /// Create a new Loro transaction.
    /// There can be only one transaction at a time.
    ///
    /// It's caller's responsibility to call `commit` or `abort` on the transaction.
    /// Transaction.free() will commit the transaction if it's not committed or aborted.
    #[wasm_bindgen(js_name = "newTransaction")]
    pub fn new_transaction(&self, origin: Option<String>) -> Transaction {
        Transaction(Some(
            self.0.txn_with_origin(&origin.unwrap_or_default()).unwrap(),
        ))
    }

    pub fn attach(&mut self) {
        self.0.attach();
    }

    pub fn checkout(&mut self, frontiers: Vec<JsID>) -> JsResult<()> {
        self.0.checkout(&ids_to_frontiers(frontiers)?)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "peerId", method, getter)]
    pub fn peer_id(&self) -> u64 {
        self.0.peer_id()
    }

    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&self, name: &str) -> JsResult<LoroText> {
        let text = self.0.get_text(name);
        Ok(LoroText(text))
    }

    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&self, name: &str) -> JsResult<LoroMap> {
        let map = self.0.get_map(name);
        Ok(LoroMap(map))
    }

    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&self, name: &str) -> JsResult<LoroList> {
        let list = self.0.get_list(name);
        Ok(LoroList(list))
    }

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
                LoroRichtext(richtext).into()
            }
        })
    }

    #[inline(always)]
    pub fn version(&self) -> Vec<u8> {
        self.0.oplog_vv().encode()
    }

    #[inline]
    pub fn frontiers(&self) -> Vec<JsID> {
        frontiers_to_ids(&self.0.oplog_frontiers())
    }

    /// - -1: self's version is less than frontiers or is parallel to target
    /// - 0: self's version equals to frontiers
    /// - 1: self's version is greater than frontiers
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

    #[wasm_bindgen(js_name = "exportSnapshot")]
    pub fn export_snapshot(&self) -> JsResult<Vec<u8>> {
        Ok(self.0.export_snapshot())
    }

    #[wasm_bindgen(skip_typescript, js_name = "exportFrom")]
    pub fn export_from(&self, version: &JsValue) -> JsResult<Vec<u8>> {
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

        Ok(self.0.export_from(&vv))
    }

    pub fn import(&self, update_or_snapshot: &[u8]) -> JsResult<()> {
        self.0.import(update_or_snapshot)?;
        Ok(())
    }

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

    // TODO: convert event and event sub config
    pub fn subscribe(&self, f: js_sys::Function) -> u32 {
        let observer = observer::Observer::new(f);
        self.0
            .subscribe_deep(Arc::new(move |e| {
                call_subscriber(observer.clone(), e);
            }))
            .into_u32()
    }

    pub fn unsubscribe(&self, subscription: u32) {
        self.0.unsubscribe(SubID::from_u32(subscription))
    }

    /// It's the caller's responsibility to commit and free the transaction
    #[wasm_bindgen(js_name = "__raw__transactionWithOrigin")]
    pub fn transaction_with_origin(
        &self,
        origin: &JsOrigin,
        f: js_sys::Function,
    ) -> JsResult<JsValue> {
        let origin = origin.as_string().unwrap();
        debug_log::group!("transaction with origin: {}", origin);
        let txn = self.0.txn_with_origin(&origin)?;
        let js_txn = JsValue::from(Transaction(Some(txn)));
        let ans = f.call1(&JsValue::NULL, &js_txn);
        debug_log::group_end!();
        ans
    }
}

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

#[wasm_bindgen]
pub struct Transaction(Option<Txn>);

#[wasm_bindgen]
impl Transaction {
    pub fn commit(&mut self) -> JsResult<()> {
        if let Some(x) = self.0.take() {
            x.commit()?;
        }
        Ok(())
    }

    pub fn abort(&mut self) -> JsResult<()> {
        if let Some(x) = self.0.take() {
            x.abort();
        }
        Ok(())
    }

    fn as_mut(&mut self) -> JsResult<&mut Txn> {
        self.0
            .as_mut()
            .ok_or_else(|| JsValue::from_str("Transaction is aborted"))
    }
}

#[wasm_bindgen]
pub struct LoroText(TextHandler);

#[wasm_bindgen]
impl LoroText {
    pub fn __txn_insert(
        &mut self,
        txn: &mut Transaction,
        index: usize,
        content: &str,
    ) -> JsResult<()> {
        self.0.insert(txn.as_mut()?, index, content)?;
        Ok(())
    }

    pub fn __txn_delete(
        &mut self,
        txn: &mut Transaction,
        index: usize,
        len: usize,
    ) -> JsResult<()> {
        self.0.delete(txn.as_mut()?, index, len)?;
        Ok(())
    }

    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.0.get_value().as_string().unwrap().to_string()
    }

    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.0.len_utf16()
    }

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

    pub fn unsubscribe(&self, loro: &Loro, subscription: u32) -> JsResult<()> {
        loro.0.unsubscribe(SubID::from_u32(subscription));
        Ok(())
    }
}

#[wasm_bindgen]
pub struct LoroMap(MapHandler);
const CONTAINER_TYPE_ERR: &str = "Invalid container type, only supports Text, Map, List";

#[wasm_bindgen]
impl LoroMap {
    pub fn __txn_insert(
        &mut self,
        txn: &mut Transaction,
        key: &str,
        value: JsValue,
    ) -> JsResult<()> {
        self.0.insert(txn.as_mut()?, key, value.into())?;
        Ok(())
    }

    pub fn __txn_delete(&mut self, txn: &mut Transaction, key: &str) -> JsResult<()> {
        self.0.delete(txn.as_mut()?, key)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> JsValue {
        self.0.get(key).into()
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> JsValue {
        let value = self.0.get_value();
        value.into()
    }

    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    #[wasm_bindgen(js_name = "getDeepValue")]
    pub fn get_value_deep(&self) -> JsValue {
        self.0.get_deep_value().into()
    }

    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(
        &mut self,
        txn: &mut Transaction,
        key: &str,
        container_type: &str,
    ) -> JsResult<JsValue> {
        let type_ = match container_type {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let t = txn.as_mut()?;
        let c = self.0.insert_container(t, key, type_)?;

        let container = match type_ {
            ContainerType::Map => LoroMap(c.into_map().unwrap()).into(),
            ContainerType::List => LoroList(c.into_list().unwrap()).into(),
            ContainerType::Text => LoroList(c.into_list().unwrap()).into(),
        };
        Ok(container)
    }

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

    #[wasm_bindgen(js_name = "size", method, getter)]
    pub fn size(&self) -> usize {
        self.0.len()
    }
}

#[wasm_bindgen]
pub struct LoroList(ListHandler);

#[wasm_bindgen]
impl LoroList {
    pub fn __txn_insert(
        &mut self,
        txn: &mut Transaction,
        index: usize,
        value: JsValue,
    ) -> JsResult<()> {
        self.0.insert(txn.as_mut()?, index, value.into())?;
        Ok(())
    }

    pub fn __txn_delete(
        &mut self,
        txn: &mut Transaction,
        index: usize,
        len: usize,
    ) -> JsResult<()> {
        self.0.delete(txn.as_mut()?, index, len)?;
        Ok(())
    }

    pub fn get(&self, index: usize) -> JsValue {
        self.0.get(index).into()
    }

    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = self.0.id().into();
        value.into()
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.0.get_value().into()
    }

    #[wasm_bindgen(js_name = "getDeepValue")]
    pub fn get_deep_value(&self) -> JsValue {
        let value = self.0.get_deep_value();
        value.into()
    }

    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(
        &mut self,
        txn: &mut Transaction,
        pos: usize,
        container: &str,
    ) -> JsResult<JsValue> {
        let _type = match container {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let t = txn.as_mut()?;
        let c = self.0.insert_container(t, pos, _type)?;
        let container = match _type {
            ContainerType::Map => LoroMap(c.into_map().unwrap()).into(),
            ContainerType::List => LoroList(c.into_list().unwrap()).into(),
            ContainerType::Text => LoroText(c.into_text().unwrap()).into(),
        };
        Ok(container)
    }

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

    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.0.len()
    }
}

#[wasm_bindgen]
pub struct LoroRichtext(TextHandler);

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
export type ContainerType = "Text" | "Map" | "List";
export type ContainerID =
  | `/${string}:${ContainerType}`
  | `${number}@${number}:${ContainerType}`;

interface Loro {
    exportFrom(version?: Uint8Array): Uint8Array;
    getContainerById(id: ContainerID): LoroText | LoroMap | LoroList;
}
"#;
