use std::sync::Arc;

use loro_internal::{
    handler::{counter::CounterHandler, Handler},
    obs::SubID,
    HandlerTrait, LoroDoc,
};
use wasm_bindgen::prelude::*;

use crate::{
    call_after_micro_task, convert::handler_to_js_value, observer, JsContainerOrUndefined,
    JsLoroTreeOrUndefined, JsResult,
};

/// The handler of a tree(forest) container.
#[derive(Clone)]
#[wasm_bindgen]
pub struct LoroCounter {
    pub(crate) handler: CounterHandler,
    pub(crate) doc: Option<Arc<LoroDoc>>,
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
            doc: None,
        }
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

    /// Get the parent container of the counter container.
    ///
    /// - The parent container of the root counter is `undefined`.
    /// - The object returned is a new js object each time because it need to cross
    ///   the WASM boundary.
    pub fn parent(&self) -> JsContainerOrUndefined {
        if let Some(p) = HandlerTrait::parent(&self.handler) {
            handler_to_js_value(p, self.doc.clone()).into()
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
            handler_to_js_value(Handler::Counter(h), self.doc.clone()).into()
        } else {
            JsValue::UNDEFINED.into()
        }
    }
}
