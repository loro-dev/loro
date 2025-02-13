use super::subscription_to_js_function_callback;
use loro_internal::{
    handler::{counter::CounterHandler, Handler},
    HandlerTrait,
};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

use crate::{
    call_after_micro_task, convert::handler_to_js_value, observer, JsContainerID,
    JsContainerOrUndefined, JsCounterStr, JsLoroTreeOrUndefined, JsResult,
};

/// The handler of a counter container.
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroCounter {
    pub(crate) handler: CounterHandler,
}

impl Default for LoroCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl LoroCounter {
    /// Create a new LoroCounter.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            handler: CounterHandler::new_detached(),
        }
    }

    /// "Counter"
    pub fn kind(&self) -> JsCounterStr {
        JsValue::from_str("Counter").into()
    }

    /// The container id of this handler.
    #[wasm_bindgen(js_name = "id", method, getter)]
    pub fn id(&self) -> JsContainerID {
        let value: JsValue = (&self.handler.id()).into();
        value.into()
    }

    /// Increment the counter by the given value.
    pub fn increment(&self, value: f64) -> JsResult<()> {
        self.handler.increment(value)?;
        Ok(())
    }

    /// Decrement the counter by the given value.
    pub fn decrement(&self, value: f64) -> JsResult<()> {
        self.handler.decrement(value)?;
        Ok(())
    }

    /// Get the value of the counter.
    #[wasm_bindgen(js_name = "value", getter)]
    pub fn get_value(&self) -> f64 {
        self.handler.get_value().into_double().unwrap()
    }

    /// Subscribe to the changes of the counter.
    pub fn subscribe(&self, f: js_sys::Function) -> JsResult<JsValue> {
        let observer = observer::Observer::new(f);
        let doc = self
            .handler
            .doc()
            .ok_or_else(|| JsError::new("Document is not attached"))?;
        let sub = doc.subscribe(
            &self.handler.id(),
            Arc::new(move |e| {
                call_after_micro_task(observer.clone(), e);
            }),
        );
        Ok(subscription_to_js_function_callback(sub))
    }

    /// Get the parent container of the counter container.
    ///
    /// - The parent container of the root counter is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = HandlerTrait::parent(&self.handler) {
            handler_to_js_value(p, false).into()
        } else {
            JsContainerOrUndefined::from(JsValue::UNDEFINED)
        }
    }

    /// Whether the container is attached to a docuemnt.
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
            handler_to_js_value(Handler::Counter(h), false).into()
        } else {
            JsValue::UNDEFINED.into()
        }
    }

    /// Get the value of the counter.
    #[wasm_bindgen(js_name = "getShallowValue")]
    pub fn get_shallow_value(&self) -> f64 {
        self.handler.get_value().into_double().unwrap()
    }
}
