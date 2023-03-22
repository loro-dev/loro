use js_sys::{Array, Object, Reflect, Uint8Array};
use loro_internal::{
    configure::{Configure, SecureRandomGenerator},
    container::{registry::ContainerWrapper, ContainerID},
    context::Context,
    log_store::GcConfig,
    ContainerType, List, LoroCore, Map, Origin, Text, Transact, TransactionWrap, VersionVector,
};
use std::{cell::RefCell, ops::Deref, sync::Arc};
use wasm_bindgen::{__rt::RefMut, prelude::*};
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
    #[wasm_bindgen(typescript_type = "String")]
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
            change: Default::default(),
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

    #[wasm_bindgen(js_name = "exportSnapshot")]
    pub fn export_snapshot(&self) -> JsResult<Vec<u8>> {
        Ok(self.0.borrow().encode_all())
    }

    #[wasm_bindgen(js_name = "importSnapshot")]
    pub fn import_snapshot(&self, input: Vec<u8>) -> JsResult<()> {
        self.0.borrow_mut().decode(&input)?;
        Ok(())
    }

    #[wasm_bindgen(skip_typescript, js_name = "exportUpdates")]
    pub fn export_updates(&self, version: &JsValue) -> JsResult<Vec<u8>> {
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

    #[wasm_bindgen(js_name = "importUpdates")]
    pub fn import_updates(&self, data: Vec<u8>) -> JsResult<()> {
        self.0.borrow_mut().decode(&data)?;
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
            observer.call1(
                // &JsValue::from_bool(e.local)
                &Event {
                    local: e.local,
                    origin: e.origin.clone(),
                }
                .into(),
            );
        }))
    }

    pub fn unsubscribe(&self, subscription: u32) {
        self.0.borrow_mut().unsubscribe_deep(subscription)
    }

    fn transaction_impl(&self, txn: TransactionWrap, f: js_sys::Function) -> JsResult<()> {
        let js_txn = JsValue::from(Transaction(txn));
        f.call1(&JsValue::NULL, &js_txn)?;
        // TODO: what is the best way to drop txn
        // Or Reference Y-crdt: https://github.com/y-crdt/y-crdt/blob/3e7450114ab3d5d4cba93eeb0710f92371e57c74/tests-wasm/testHelper.js#L6
        let ptr = Reflect::get(&js_txn, &JsValue::from_str("ptr"))?;
        let ptr = ptr.as_f64().ok_or(JsValue::NULL).unwrap() as u32;
        use wasm_bindgen::convert::FromWasmAbi;
        drop(unsafe { Transaction::from_abi(ptr) });
        Ok(())
    }

    pub fn transaction(&self, f: js_sys::Function) -> JsResult<()> {
        let txn = self.0.borrow().transact();
        self.transaction_impl(txn, f)
    }

    #[wasm_bindgen(js_name = "transactionWithOrigin")]
    pub fn transaction_with_origin(&self, origin: &JsOrigin, f: js_sys::Function) -> JsResult<()> {
        let origin = origin.as_string().map(Origin::from);
        let txn = self.0.borrow().transact_with(origin);
        self.transaction_impl(txn, f)
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub struct Event {
    pub local: bool,
    origin: Option<Origin>,
}

#[wasm_bindgen]
impl Event {
    #[wasm_bindgen(js_name = "origin", method, getter)]
    pub fn origin(&self) -> Option<JsOrigin> {
        self.origin
            .as_ref()
            .map(|o| JsValue::from_str(o.as_str()).into())
    }
}

#[wasm_bindgen]
pub struct Transaction(TransactionWrap);

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
    pub fn insert(&mut self, txn: &JsTransaction, index: usize, content: &str) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.insert_utf16(&txn, index, content)?;
        Ok(())
    }

    pub fn delete(&mut self, txn: &JsTransaction, index: usize, len: usize) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.delete_utf16(&txn, index, len)?;
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
}

#[wasm_bindgen]
pub struct LoroMap(Map);

#[wasm_bindgen]
impl LoroMap {
    #[wasm_bindgen(js_name = "set")]
    pub fn insert(&mut self, txn: &JsTransaction, key: &str, value: JsValue) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(&txn, key, v)?;
        } else {
            self.0.insert(&txn, key, value)?;
        };
        Ok(())
    }

    pub fn delete(&mut self, txn: &JsTransaction, key: &str) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.delete(&txn, key)?;
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
        let _type = match container_type {
            "text" => ContainerType::Text,
            "map" => ContainerType::Map,
            "list" => ContainerType::List,
            _ => {
                return Err(JsValue::from_str(
                    "Invalid container type, only supports text, map, list",
                ))
            }
        };
        let idx = self.0.insert(&txn, key, _type)?.unwrap();

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
}

#[wasm_bindgen]
pub struct LoroList(List);

#[wasm_bindgen]
impl LoroList {
    pub fn insert(&mut self, txn: &JsTransaction, index: usize, value: JsValue) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(&txn, index, v)?;
        } else {
            self.0.insert(&txn, index, value)?;
        };
        Ok(())
    }

    pub fn delete(&mut self, txn: &JsTransaction, index: usize, len: usize) -> JsResult<()> {
        let txn = get_transaction_mut(txn);
        self.0.delete(&txn, index, len)?;
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

    /// FIXME: the returned value should not hold a Arc reference to the container
    /// it may cause memory leak
    #[wasm_bindgen(js_name = "insertContainer")]
    pub fn insert_container(
        &mut self,
        txn: &JsTransaction,
        pos: usize,
        container: &str,
    ) -> JsResult<JsValue> {
        let txn = get_transaction_mut(txn);
        let _type = match container {
            "text" => ContainerType::Text,
            "map" => ContainerType::Map,
            "list" => ContainerType::List,
            _ => return Err(JsValue::from_str("Invalid container type")),
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
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
export type ContainerType = "Text" | "Map" | "List";
export type ContainerID = { id: string; type: ContainerType } | {
  root: string;
  type: ContainerType;
};

interface Loro {
    exportUpdates(version?: Uint8Array): Uint8Array;
    getContainerById(id: ContainerID): LoroText | LoroMap | LoroList;
    transaction(callback: (txn: Transaction)=>void): void;
    transactionWithOrigin(origin: string, callback: (txn: Transaction)=>void): void;
}
"#;
