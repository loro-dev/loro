use thiserror::Error;

use crate::id::ClientID;

#[derive(Error, Debug)]
pub enum LoroError {
    #[error("Context's client_id({found:?}) does not match Container's client_id({expected:?})")]
    UnmatchedContext { expected: ClientID, found: ClientID },
    // #[error("the data for key `{0}` is not available")]
    // Redaction(String),
    // #[error("invalid header (expected {expected:?}, found {found:?})")]
    // InvalidHeader { expected: String, found: String },
    // #[error("unknown data store error")]
    // Unknown,
    #[error("Decode error.")]
    DecodeError(Box<str>),
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use wasm_bindgen::JsValue;

    use crate::LoroError;

    impl From<LoroError> for JsValue {
        fn from(value: LoroError) -> Self {
            JsValue::from_str(&value.to_string())
        }
    }
}
