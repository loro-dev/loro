use thiserror::Error;

use crate::{container::ContainerID, ContainerType};

#[derive(Error, Debug)]
pub enum LoroError {
    #[error("Expect container with the id of {id:?} has type {expected_type:?} but the actual type is {actual_type:?}.")]
    ContainerTypeError {
        id: ContainerID,
        actual_type: ContainerType,
        expected_type: ContainerType,
    },
    // #[error("the data for key `{0}` is not available")]
    // Redaction(String),
    // #[error("invalid header (expected {expected:?}, found {found:?})")]
    // InvalidHeader { expected: String, found: String },
    // #[error("unknown data store error")]
    // Unknown,
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
