use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use loro_internal::{
    configure::{Configure, SecureRandomGenerator},
    container::{registry::ContainerWrapper, ContainerID},
    context::Context,
    event::{Diff, Path},
    log_store::GcConfig,
    version::Frontiers,
    ContainerType, List, LoroCore, Map, Origin, Text, Transact, TransactionWrap, VersionVector,
};
use std::{borrow::Cow, cell::RefCell, cmp::Ordering, ops::Deref, rc::Rc, sync::Arc};
use wasm_bindgen::{
    __rt::{IntoJsResult, RefMut},
    prelude::*,
};
mod log;
mod prelim;
pub use prelim::{PrelimList, PrelimMap, PrelimText};

use crate::convert::js_try_to_prelim;
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

type JsResult<T> = Result<T, JsValue>;

#[wasm_bindgen]
pub struct Loro(RefCell<LoroCore>);

impl Deref for Loro {
    type Target = RefCell<LoroCore>;

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

        pub fn call1(&self, arg: &JsValue) {
            if std::thread::current().id() == self.thread {
                self.f.call1(&JsValue::NULL, arg).unwrap();
            } else {
                panic!("Observer called from different thread")
            }
        }
    }

    unsafe impl Send for Observer {}
}

#[wasm_bindgen]
impl Loro {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let cfg: Configure = Configure {
            gc: GcConfig::default().with_gc(false),
            get_time: || js_sys::Date::now() as i64,
            rand: Arc::new(MathRandom),
        };
        Self(RefCell::new(LoroCore::new(cfg, None)))
    }

    #[wasm_bindgen(js_name = "clientId", method, getter)]
    pub fn client_id(&self) -> u64 {
        self.0.borrow().client_id()
    }

    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&self, name: &str) -> JsResult<LoroText> {
        let text = self.0.borrow_mut().get_text(name);
        Ok(LoroText(text))
    }

    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&self, name: &str) -> JsResult<LoroMap> {
        let map = self.0.borrow_mut().get_map(name);
        Ok(LoroMap(map))
    }

    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&self, name: &str) -> JsResult<LoroList> {
        let list = self.0.borrow_mut().get_list(name);
        Ok(LoroList(list))
    }

    #[wasm_bindgen(skip_typescript, js_name = "getContainerById")]
    pub fn get_container_by_id(&self, container_id: JsContainerID) -> JsResult<JsValue> {
        let container_id: ContainerID = container_id.to_owned().try_into()?;
        let ty = container_id.container_type();
        let container = self.0.borrow_mut().get_container(&container_id);
        if let Some(container) = container {
            let client_id = self.0.borrow().client_id();
            Ok(match ty {
                ContainerType::Text => {
                    let text: Text = Text::from_instance(container, client_id);
                    LoroText(text).into()
                }
                ContainerType::Map => {
                    let map: Map = Map::from_instance(container, client_id);
                    LoroMap(map).into()
                }
                ContainerType::List => {
                    let list: List = List::from_instance(container, client_id);
                    LoroList(list).into()
                }
            })
        } else {
            Err(JsValue::from_str("Container not found"))
        }
    }

    #[inline(always)]
    pub fn version(&self) -> Vec<u8> {
        self.0.borrow().vv_cloned().encode()
    }

    #[inline]
    pub fn frontiers(&self) -> Vec<u8> {
        self.0.borrow().frontiers().encode()
    }

    /// - -1: self's version is less than frontiers or is parallel to target
    /// - 0: self's version equals to frontiers
    /// - 1: self's version is greater than frontiers
    #[inline]
    #[wasm_bindgen(js_name = "cmpFrontiers")]
    pub fn cmp_frontiers(&self, frontiers: &[u8]) -> JsResult<i32> {
        let frontiers = Frontiers::decode(frontiers)?;
        Ok(match self.0.borrow().cmp_frontiers(&frontiers) {
            Ordering::Less => -1,
            Ordering::Greater => 1,
            Ordering::Equal => 0,
        })
    }

    #[wasm_bindgen(js_name = "exportSnapshot")]
    pub fn export_snapshot(&self) -> JsResult<Vec<u8>> {
        Ok(self.0.borrow().encode_all())
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

        Ok(self.0.borrow().encode_from(vv))
    }

    pub fn import(&self, update_or_snapshot: &[u8]) -> JsResult<()> {
        self.0.borrow_mut().decode(update_or_snapshot)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "importUpdateBatch")]
    pub fn import_update_batch(&self, data: Array) -> JsResult<()> {
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
        Ok(self.0.borrow_mut().decode_batch(&data)?)
    }

    #[wasm_bindgen(js_name = "toJson")]
    pub fn to_json(&self) -> JsResult<JsValue> {
        let json = self.0.borrow().to_json();
        Ok(json.into())
    }

    // TODO: convert event and event sub config
    pub fn subscribe(&self, f: js_sys::Function) -> u32 {
        let observer = observer::Observer::new(f);
        self.0.borrow_mut().subscribe_deep(Box::new(move |e| {
            call_after_micro_task(observer.clone(), e);
        }))
    }

    pub fn unsubscribe(&self, subscription: u32) {
        self.0.borrow_mut().unsubscribe_deep(subscription)
    }

    /// It's the caller's responsibility to commit and free the transaction
    #[wasm_bindgen(js_name = "__raw__transactionWithOrigin")]
    pub fn transaction_with_origin(&self, origin: &JsOrigin, f: js_sys::Function) -> JsResult<()> {
        let origin = origin.as_string().map(Origin::from);
        let txn = self.0.borrow().transact_with(origin);
        let js_txn = JsValue::from(Transaction(txn));
        f.call1(&JsValue::NULL, &js_txn)?;
        Ok(())
    }
}

fn call_after_micro_task(ob: observer::Observer, e: &loro_internal::event::Event) {
    let e = e.clone();
    let promise = Promise::resolve(&JsValue::NULL);
    type C = Closure<dyn FnMut(JsValue)>;
    let drop_handler: Rc<RefCell<Option<C>>> = Rc::new(RefCell::new(None));
    let copy = drop_handler.clone();
    let closure = Closure::once(move |_: JsValue| {
        ob.call1(
            &Event {
                local: e.local,
                origin: e.origin.clone(),
                target: e.target.clone(),
                diff: Either::A(e.diff),
                path: Either::A(e.absolute_path.clone()),
            }
            .into(),
        );

        drop(copy);
    });
    let _ = promise.then(&closure);
    drop_handler.borrow_mut().replace(closure);
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}

enum Either<A, B> {
    A(A),
    B(B),
}

#[wasm_bindgen]
pub struct Event {
    pub local: bool,
    origin: Option<Origin>,
    target: ContainerID,
    diff: Either<Diff, JsValue>,
    path: Either<Path, JsValue>,
}

#[wasm_bindgen]
impl Event {
    #[wasm_bindgen(js_name = "origin", method, getter)]
    pub fn origin(&self) -> Option<JsOrigin> {
        self.origin
            .as_ref()
            .map(|o| JsValue::from_str(o.as_str()).into())
    }

    #[wasm_bindgen(getter)]
    pub fn target(&self) -> JsContainerID {
        JsValue::from_str(&self.target.to_string()).into()
    }

    #[wasm_bindgen(getter)]
    pub fn path(&mut self) -> JsValue {
        match &mut self.path {
            Either::A(path) => {
                let arr = Array::new_with_length(path.len() as u32);
                for (i, p) in path.iter().enumerate() {
                    arr.set(i as u32, p.clone().into());
                }
                let inner: JsValue = arr.into_js_result().unwrap();
                self.path = Either::B(inner.clone());
                inner
            }
            Either::B(new) => new.clone(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn diff(&mut self) -> JsValue {
        match &self.diff {
            Either::A(diff) => {
                let value: JsValue = diff.clone().into();
                self.diff = Either::B(value.clone());
                value
            }
            Either::B(ans) => ans.clone(),
        }
    }
}

#[wasm_bindgen]
pub struct Transaction(TransactionWrap);

#[wasm_bindgen]
impl Transaction {
    pub fn commit(&self) -> JsResult<()> {
        self.0.commit()?;
        Ok(())
    }
}

fn get_transaction_mut(txn: &JsTransaction) -> TransactionWrap {
    use wasm_bindgen::convert::RefMutFromWasmAbi;
    let js: &JsValue = txn.as_ref();
    if js.is_undefined() || js.is_null() {
        panic!("you should input Transaction");
    } else {
        let ctor_name = Object::get_prototype_of(js).constructor().name();
        if ctor_name == "Transaction" {
            let ptr = Reflect::get(js, &JsValue::from_str("ptr")).unwrap();
            let ptr = ptr.as_f64().ok_or(JsValue::NULL).unwrap() as u32;
            let txn: RefMut<Transaction> = unsafe { Transaction::ref_mut_from_abi(ptr) };
            txn.0.transact()
        } else if ctor_name == "Loro" {
            let ptr = Reflect::get(js, &JsValue::from_str("ptr")).unwrap();
            let ptr = ptr.as_f64().ok_or(JsValue::NULL).unwrap() as u32;
            let loro: RefMut<Loro> = unsafe { Loro::ref_mut_from_abi(ptr) };
            let loro = loro.0.borrow();
            loro.transact()
        } else {
            panic!("you should input Transaction");
        }
    }
}

#[wasm_bindgen]
pub struct LoroText(Text);

#[wasm_bindgen]
impl LoroText {
    pub fn __loro_insert(&mut self, txn: &Loro, index: usize, content: &str) -> JsResult<()> {
        self.0.insert_utf16(&*txn.0.borrow(), index, content)?;
        Ok(())
    }

    pub fn __loro_delete(&mut self, txn: &Loro, index: usize, len: usize) -> JsResult<()> {
        self.0.delete_utf16(&*txn.0.borrow(), index, len)?;
        Ok(())
    }

    pub fn __txn_insert(&mut self, txn: &Transaction, index: usize, content: &str) -> JsResult<()> {
        self.0.insert_utf16(&txn.0, index, content)?;
        Ok(())
    }

    pub fn __txn_delete(&mut self, txn: &Transaction, index: usize, len: usize) -> JsResult<()> {
        self.0.delete_utf16(&txn.0, index, len)?;
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
        self.0.len()
    }

    pub fn subscribe(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    #[wasm_bindgen(js_name = "subscribeOnce")]
    pub fn subscribe_once(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe_once(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    pub fn unsubscribe(&self, txn: &JsTransaction, subscription: u32) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.unsubscribe(&txn, subscription)?;
        Ok(())
    }
}

#[wasm_bindgen]
pub struct LoroMap(Map);
const CONTAINER_TYPE_ERR: &str = "Invalid container type, only supports Text, Map, List";

#[wasm_bindgen]
impl LoroMap {
    pub fn __loro_insert(&mut self, txn: &Loro, key: &str, value: JsValue) -> JsResult<()> {
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(&*txn.0.borrow(), key, v)?;
        } else {
            self.0.insert(&*txn.0.borrow(), key, value)?;
        };
        Ok(())
    }

    pub fn __txn_insert(&mut self, txn: &Transaction, key: &str, value: JsValue) -> JsResult<()> {
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(&txn.0, key, v)?;
        } else {
            self.0.insert(&txn.0, key, value)?;
        };
        Ok(())
    }

    pub fn __loro_delete(&mut self, txn: &Loro, key: &str) -> JsResult<()> {
        self.0.delete(&*txn.0.borrow(), key)?;
        Ok(())
    }

    pub fn __txn_delete(&mut self, txn: &Transaction, key: &str) -> JsResult<()> {
        self.0.delete(&txn.0, key)?;
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

    #[wasm_bindgen(js_name = "getValueDeep")]
    pub fn get_value_deep(&self, ctx: &Loro) -> JsValue {
        self.0.get_value_deep(ctx.deref()).into()
    }

    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(
        &mut self,
        txn: &JsTransaction,
        key: &str,
        container_type: &str,
    ) -> JsResult<JsValue> {
        let txn = get_transaction_mut(txn);
        let type_ = match container_type {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let idx = self.0.insert(&txn, key, type_)?.unwrap();

        let container = match type_ {
            ContainerType::Text => {
                let x = txn.get_text_by_idx(idx).unwrap();
                LoroText(x).into()
            }
            ContainerType::Map => {
                let x = txn.get_map_by_idx(idx).unwrap();
                LoroMap(x).into()
            }
            ContainerType::List => {
                let x = txn.get_list_by_idx(idx).unwrap();
                LoroList(x).into()
            }
        };
        Ok(container)
    }

    pub fn subscribe(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    #[wasm_bindgen(js_name = "subscribeOnce")]
    pub fn subscribe_once(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe_once(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    #[wasm_bindgen(js_name = "subscribeDeep")]
    pub fn subscribe_deep(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe_deep(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    pub fn unsubscribe(&self, txn: &JsTransaction, subscription: u32) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.unsubscribe(&txn, subscription)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "size", method, getter)]
    pub fn size(&self) -> usize {
        self.0.len()
    }
}

#[wasm_bindgen]
pub struct LoroList(List);

#[wasm_bindgen]
impl LoroList {
    pub fn __loro_insert(&mut self, loro: &Loro, index: usize, value: JsValue) -> JsResult<()> {
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(&*loro.0.borrow(), index, v)?;
        } else {
            self.0.insert(&*loro.0.borrow(), index, value)?;
        };
        Ok(())
    }

    pub fn __txn_insert(
        &mut self,
        txn: &Transaction,
        index: usize,
        value: JsValue,
    ) -> JsResult<()> {
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(&txn.0, index, v)?;
        } else {
            self.0.insert(&txn.0, index, value)?;
        };
        Ok(())
    }

    pub fn __loro_delete(&mut self, loro: &Loro, index: usize, len: usize) -> JsResult<()> {
        self.0.delete(&*loro.0.borrow(), index, len)?;
        Ok(())
    }

    pub fn __txn_delete(&mut self, loro: &Transaction, index: usize, len: usize) -> JsResult<()> {
        self.0.delete(&loro.0, index, len)?;
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

    #[wasm_bindgen(js_name = "getValueDeep")]
    pub fn get_value_deep(&self, ctx: &Loro) -> JsValue {
        let value = self.0.get_value_deep(ctx.deref());
        value.into()
    }

    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(
        &mut self,
        txn: &JsTransaction,
        pos: usize,
        container: &str,
    ) -> JsResult<JsValue> {
        let txn = get_transaction_mut(txn);
        let _type = match container {
            "text" | "Text" => ContainerType::Text,
            "map" | "Map" => ContainerType::Map,
            "list" | "List" => ContainerType::List,
            _ => return Err(JsValue::from_str(CONTAINER_TYPE_ERR)),
        };
        let idx = self.0.insert(&txn, pos, _type)?.unwrap();
        let container = match _type {
            ContainerType::Text => {
                let x = txn.get_text_by_idx(idx).unwrap();
                LoroText(x).into()
            }
            ContainerType::Map => {
                let x = txn.get_map_by_idx(idx).unwrap();
                LoroMap(x).into()
            }
            ContainerType::List => {
                let x = txn.get_list_by_idx(idx).unwrap();
                LoroList(x).into()
            }
        };
        Ok(container)
    }

    pub fn subscribe(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    #[wasm_bindgen(js_name = "subscribeOnce")]
    pub fn subscribe_once(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe_once(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    #[wasm_bindgen(js_name = "subscribeDeep")]
    pub fn subscribe_deep(&self, txn: &JsTransaction, f: js_sys::Function) -> JsResult<u32> {
        let observer = observer::Observer::new(f);
        let txn = get_transaction_mut(txn);
        let ans = self.0.subscribe_deep(
            &txn,
            Box::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        )?;
        Ok(ans)
    }

    pub fn unsubscribe(&self, txn: &JsTransaction, subscription: u32) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.unsubscribe(&txn, subscription)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "length", method, getter)]
    pub fn length(&self) -> usize {
        self.0.len()
    }
}

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
